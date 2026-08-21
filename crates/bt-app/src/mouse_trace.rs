//! **`BT_MOUSE_TRACE` — one named file, one line per station on a click's road.**
//!
//! A click on a hyperlink in a terminal pane travels through six surfaces before
//! anything opens: the window router, the chrome's own hit test, the mouse route
//! a press arms, the release that spends what the press promised, the activation
//! table, and the landing rule that has to mint a pane for the file to arrive in.
//! Every one of those can decline, and — until this file existed — every one of
//! them declined **silently**. A user on a second monitor reporting "the click
//! does nothing" was reporting the absence of six different possible sentences,
//! and no build could tell which one was missing.
//!
//! So this is forensic apparatus and nothing else: **it changes no behaviour**.
//! Every station writes what it decided and why, and the file is the transcript
//! of a gesture that can be carried back from a machine we do not have.
//!
//! **Named like [`BT_PTY_DUMP`](bt_pty) — the value is a *file*, not a folder.**
//! Handing a directory to that one reports Access denied dressed up as a ConPTY
//! failure; this one says so plainly on stderr and then stays off, because a
//! diagnostic that takes the program down with it is worse than the silence it
//! was built to end.
//!
//! **Off costs one atomic load**, and the file, the clock and the closure gate
//! all live in [`crate::trace`] now — this module is the variable's name, its
//! header, and the stations that call it.

use crate::trace::Gate;
pub use crate::trace::{Trace, emit};

/// The variable that names the file, and the first line of every opened trace so
/// that a file which has collected several runs can still be told what it is and
/// where each run began.
static GATE: Gate = Gate::new(
    "BT_MOUSE_TRACE",
    "# BT_MOUSE_TRACE_V1 elapsed_ms event field=value…",
);

/// The process's trace, opening it on first ask.
pub fn global() -> Option<&'static Trace> {
    GATE.get()
}

/// Whether anything is listening — for the handful of call sites that must
/// *compute* a field (a hit test, say) rather than merely format one.
pub fn is_on() -> bool {
    global().is_some()
}

/// [`emit`] against the process's own trace — what every station calls.
pub fn line(message: impl FnOnce() -> String) {
    emit(global(), message);
}
