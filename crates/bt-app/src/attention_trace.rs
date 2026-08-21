//! **`BT_ATTENTION_TRACE` — one named file, one line per decision the attention
//! queue makes** (§7.1.5b P1-8).
//!
//! The queue has exactly three moving parts and all three of them are invisible:
//! a bell latches inside `bt-term`, a ticket is handed out (or refused) inside a
//! per-frame pass, and a ticket is given up by an `Enter` typed into one shell.
//! A user reporting "the orange ring never goes out" is reporting the *result* of
//! those three, and no build could say which of them had happened — the ring is
//! the only thing the queue ever says out loud, and it says the same word for
//! every reason it could be lit.
//!
//! So this is forensic apparatus and nothing else: **it changes no behaviour.**
//! Same shape as [`BT_MOUSE_TRACE`](crate::mouse_trace) and for the same reasons —
//! the value is a *file* and not a folder, it is appended rather than truncated,
//! every line is flushed, and an unset variable formats nothing at all. The
//! machinery is [`crate::trace`]'s; this module is the variable's name and the
//! vocabulary its stations write.
//!
//! **The vocabulary, and why it is written in decisions rather than in events.**
//! The pass that admits to the queue runs on every turn of the event loop, and a
//! latched bell on a background tab stays latched for as long as that tab stays
//! shut. A station that spoke whenever it *saw* a latch would therefore write the
//! same line sixty times a second for an hour. So every station below fires on a
//! **change**, which for this mechanism is the same thing as an event: a latch is
//! set once per `BEL` and retired once per look, and a ticket is handed out once
//! and given up once.
//!
//! * `admit tab=<i> seat=<n> ticket=<k> bell=1 active=<0|1> focused=<0|1>` — a
//!   bell rang somewhere nobody was looking, and it took a place in the queue.
//! * `refuse tab=<i> seat=<n> reason=watched active=1 focused=1` — a bell rang in
//!   the pane the user is sitting in front of. **This is the fix of 2026-08-21**:
//!   the same look that spends the latch now also refuses the ticket, so the pane
//!   you are working in cannot hand itself a badge telling you to look at it.
//! * `look tab=<i> seat=<n> ticket=<k> kept=1` — the user switched to a tab that
//!   holds a place; the latch retired and the place did not. §7.1.5b's 看一眼阻塞
//!   的 agent 不解除阻塞, printed.
//! * `answer tab=<i> seat=<n> ticket=<k> door=enter` — the one door out.
//! * `claim tab=<i> was=<claim> now=<claim>` — what the pass above did to the dot
//!   the tab wears. Emitted only when the pass changed it, which is the only kind
//!   of claim change this file is about.
//! * `jump queue=<k> from=<ticket|none> to=tab=<i>,seat=<n>,ticket=<k>` —
//!   `Ctrl+Shift+A` walking the queue, which consumes nothing.

use crate::trace::Gate;
pub use crate::trace::{Trace, emit};

static GATE: Gate = Gate::new(
    "BT_ATTENTION_TRACE",
    "# BT_ATTENTION_TRACE_V1 elapsed_ms event field=value…",
);

/// The process's trace, opening it on first ask.
pub fn global() -> Option<&'static Trace> {
    GATE.get()
}

/// [`emit`] against the process's own trace — what the window's own verbs call.
///
/// The per-frame pass is handed its trace instead of reaching for this, because
/// its stations are the ones worth pinning line by line and a `static` read out
/// of the environment cannot be set from a test without racing every other test
/// in the process.
pub fn line(message: impl FnOnce() -> String) {
    emit(global(), message);
}
