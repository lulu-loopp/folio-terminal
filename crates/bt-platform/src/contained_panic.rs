//! **A unit of work whose panic is its own failure, not the process's end** (H2-NITS round 5;
//! T-PANIC-ROWS-HONEST) — the one unwind boundary of the product's workers and callbacks.
//!
//! The resident application installs a fatal panic hook (`bt-app`'s `install_panic_log_hook`):
//! it writes the report and then ends the process from inside the hook, before any unwinding, so
//! a bare `std::panic::catch_unwind` (and every `Drop` on the way to it) is reached only where no
//! such hook is installed. The hook asks one question before it ends anything — `bt-app`'s
//! `the_hook_lets_this_panic_unwind` — and [`is_contained`] is half of its answer: [`contain`]
//! marks this thread for the length of `work` and is its unwind boundary, so a panic inside it is
//! logged, unwinds to here and is answered as `Err`, which the caller turns into the failure its
//! `# completions` row names (C2, C3). A panic outside every `contain` ends the process.
//!
//! **What may be contained**: work that holds no state another thread reads half-written, whose
//! caller has a value to give for its failure. No other worker or product catch exists outside
//! this boundary but `bt-math`'s render boundary, which the hook honours separately through
//! `render_panic_is_contained` (`crates/bt-source/tests/contained_panic.rs` holds `bt-app`,
//! `bt-platform` and `bt-pty` to it, by a call edge to this item).

use std::cell::Cell;

thread_local! {
    static CONTAINED: Cell<bool> = const { Cell::new(false) };
}

/// Run `work` with this thread's panics contained; `Err` carries the panic, whose payload is for
/// diagnostics only (the hook has already written its report to the panic log).
pub fn contain<T>(work: impl FnOnce() -> T) -> std::thread::Result<T> {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            CONTAINED.set(self.0);
        }
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _restore = Restore(CONTAINED.replace(true));
        work()
    }))
}

/// Whether this thread is inside [`contain`] now.
#[must_use]
pub fn is_contained() -> bool {
    CONTAINED.get()
}
