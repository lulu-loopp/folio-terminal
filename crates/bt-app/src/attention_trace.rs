//! **`BT_ATTENTION_TRACE` — one named file, one line per decision the attention
//! queue makes** (§7.1.5b P1-8).
//!
//! Everything the queue does is invisible: a credential goes up inside a pane's own account, a
//! place is handed out (or refused) inside a per-frame pass, and a place is given up by an action
//! typed into one shell. A user reporting "the orange ring never goes out" is reporting the
//! *result* of those, and no build could say which of them had happened — the ring is the only
//! thing the queue ever says out loud, and it says the same word for every reason it could be lit.
//!
//! So this is forensic apparatus and nothing else: **it changes no behaviour.**
//! Same shape as [`BT_MOUSE_TRACE`](crate::mouse_trace) and for the same reasons —
//! the value is a *file* and not a folder, it is appended rather than truncated,
//! every line is flushed, and an unset variable formats nothing at all. The
//! machinery is [`crate::trace`]'s; this module is the variable's name, and the vocabulary its
//! stations write is `docs/plans/attention/plan.md` §11.1.5 — **written down there, formatted by
//! [`crate::attention`], and emitted here.** The three-way split is what lets a test assert on the
//! exact bytes of a line without driving a terminal.
//!
//! **Why it is written in decisions rather than in events.** The pass runs on every turn of the
//! event loop, and a standing request on a background tab stays standing for as long as that tab
//! stays shut. A station that spoke whenever it *saw* a level would write the same line sixty times
//! a second for an hour. So every line below is emitted on a **decision**: a program restating
//! `RequestAttention=yes` every second, or re-sending the same `wait` every second, writes nothing
//! at all, because it has changed nothing.
//!
//! **Every line names the request it is about** — `episode=<e>`, or `episode=-` for the lines that
//! honestly belong to no request. The verbs, and which of the ledger's four states each can happen
//! in, are §11.1.5's field contract; the two that matter most to a reader are that
//! `withdraw`/`expire` always carry a `ticket=` and `clear`/`drop`/`mint` never do.
//!
//! Two stations are the window's own rather than the ledger's, and are formatted here:
//!
//! * `claim tab=<i> episode=<e|-> was=<claim> now=<claim>` — what the pass did to the dot the tab
//!   wears. Emitted only when the pass changed it, which is the only kind of claim change this file
//!   is about; the `episode=` is the request behind it, and `-` is the common and honest answer,
//!   because most claim changes are unread, bell or work in flight and belong to no request.
//! * `jump queue=<k> from=<ticket|none> to=tab=<i>,seat=<n>,ticket=<k>` —
//!   `Ctrl+Shift+A` walking the queue, which consumes nothing.
//!
//! **`look` was a station here until A2 and is gone**, which is worth saying rather than deleting
//! in silence: it recorded a look retiring a bell latch while a place stood, and it existed because
//! the *same byte* fed both. It no longer does. A look spends latches, the ledger holds none, and
//! the arrival that reports one to it decides nothing — so there is nothing left for that line to
//! report (`attention` plan §10.9).

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
