//! **A worker's waits** — the thread door's first worker-side door
//! (`docs/plans/design/thread-door-2026-09-26.md` (j)13, brought forward from
//! A2c by 0.4.6 ticket U-28).
//!
//! A wait on a worker is not a stall of a window, but it is still a wait, and
//! the bare-site gate holds every one of them to a door: this module is where
//! a worker that polls — the update applier waiting for the old build's claim
//! and for the trial's receipt — does its sleeping. The capability it asks for
//! is the worker's own [`WorkerCtx`], which only the thread door and
//! `admission::enter_standalone_main` lend, so a window thread cannot call it.
//!
//! Registered in `crates/bt-app/src/window_waits.tsv`'s `# effects` section as
//! kind `worker-door-body`, authority `WorkerCtx`, with no admission identity
//! (the admission identities of `# doors` are the window thread's).

use std::time::Duration;

use crate::admission::WorkerCtx;

/// **Sleep `duration` on the worker `_worker` lends** — one effect, nothing
/// else: the caller owns the deadline it is sleeping towards, and asks again.
pub fn sleep_within(_worker: &WorkerCtx, duration: Duration) {
    std::thread::sleep(duration);
}
