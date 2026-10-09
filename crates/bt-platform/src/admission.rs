//! **The thread door**, and the admission vocabulary it lends from.
//!
//! The vocabulary — a thread's role, the window thread's phase, the door registry, the admission
//! and its token, the worker capability — is `bt-effects`' (`bt_effects::admission`), re-exported
//! here whole, so every `bt_platform::admission::…` path names it. What this module adds is the
//! one part that needs the platform: [`spawn_at_priority`] (re-exported at the crate root) starts
//! a thread already in its scheduling band and makes it a worker through
//! [`lend_worker`], lending its body the [`WorkerCtx`] a worker-only door takes (A1b). The one such
//! door today is the hand-off's (`ShellThread::enter`).
#![forbid(unsafe_code)]

pub use bt_effects::admission::*;

/// **Start a named thread that is already in its band, and lend its body the capability that says
/// it is a worker** — the thread door (`docs/ARCHITECTURE.md` §6, `docs/RULES.md` rows 52 and 53;
/// design note `docs/plans/design/thread-door-2026-09-26.md` §2).
///
/// Inside the new thread, in this order: the band, as the first statement; the role
/// [`Role::Worker`] `(name)`; then a [`WorkerCtx`] built on the thread's own stack and lent to
/// `body` by reference. `body` cannot keep the capability past its return (it is lent, and the
/// result type is chosen outside the loan), cannot send it to another thread (`!Sync`), and nothing
/// but this door and [`enter_standalone_main`] can make one: the role-entering function both call,
/// [`lend_worker`], is `bt-effects`' and public for this door alone, and `bt-app`'s window-waits
/// guard holds the product to those two callers.
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
        let ctx = bt_effects::admission::lend_worker(name);
        body(&ctx)
    })
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
