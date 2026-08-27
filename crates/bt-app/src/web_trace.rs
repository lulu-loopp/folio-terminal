//! **`BT_WEB_TRACE` — one named file, one line for every question the engine
//! asked this window about a navigation and every answer it gave back.**
//!
//! Born of a report no other instrument in this repository could answer
//! (2026-08-25): a local `.pdf` opened from the files column brought up the
//! engine's own PDF viewer — toolbar, page counter, everything — and drew
//! `ERR_FILE_NOT_FOUND` in the body, while a local `.html` down the very same
//! lane opened fine. From outside, three completely different mechanisms make
//! that one picture:
//!
//! * the [`crate::webnav`] gate refused a URI the viewer asked for, because the
//!   viewer's own request is not the one string the seat minted;
//! * the download door cancelled the navigation and turned a page into a
//!   hand-off;
//! * the engine really did fail to read the file, and the address it was given
//!   is the thing to look at.
//!
//! The four stations below tell those apart by **printing the URI sequence**,
//! which is the one fact all three stories disagree about:
//!
//! * `navigate tab=<n> seat=<n> url=<…> mint=<…>` — what the host issued, and
//!   under which mint. Written where the mint is installed, so a navigation the
//!   host refused to issue to itself is a line that stops here.
//! * `navigation_starting tab=<n> seat=<n> uri=<…> mint=<…> verdict=<…>` —
//!   **inside the gate**, which is the only place the verdict exists: the engine
//!   reports that a navigation was cancelled and never why.
//! * `navigation_completed tab=<n> seat=<n> uri=<…> success=<0|1> status=<n>` —
//!   `COREWEBVIEW2_WEB_ERROR_STATUS` as the number it is, because that is what
//!   separates "the gate cancelled it" (`ConnectionAborted`) from "the disk said
//!   no".
//! * `download_starting tab=<n> seat=<n> uri=<…> file=<…>` — the door that
//!   cancels unconditionally, so that a page that turned into a download says so
//!   in one line instead of being inferred from a blank pane.
//!
//! # The fifth station answers a different empty pane (§7.14b, 2026-08-25)
//!
//! * `place tab=<n> seat=<n> floated=<id|-> body=<rect|none> presence=<…>
//!   above=<n|-> obstructed=<0|1> carded=<0|1> front=<0|1>` — the per-frame
//!   placement's own decision. The four stations above say what the *engine* was
//!   asked and answered; this one says whether the window ever put the answer on
//!   the glass, which is the other half of "why is this pane empty" and the half
//!   a popped-out page came up on the wrong side of.
//!
//!   `above=` is [`bt_render::WebHole::above`] — **which overlay layer the hole
//!   stands on**, `-` for the seats' own place under the whole stack — and it is
//!   here because the field's absence cost a second ticket (§7.14c). On the
//!   report of 2026-08-25 this station printed `floated=1`, a body that matched
//!   the float to the pixel and `presence=Shown`, and the page was still
//!   invisible: everything the trace could see was right, and the one thing it
//!   could not see was that the hole was punched under the very window it
//!   belonged to.
//!
//! **Written on a change and never otherwise**, which is
//! [`crate::attention_trace`]'s rule for its reason exactly: a page standing
//! still is sixty identical frames a second, and a file that wrote all of them
//! would bury the one frame the reader is looking for. The gate is what the seat
//! was last *asked* ([`crate::webhost::WebSeat::wanted`]), so the line is written
//! at the moment the answer moves.
//!
//! Same apparatus and same five properties as [`crate::mouse_trace`],
//! [`crate::attention_trace`] and [`crate::preview_trace`]: the value names a
//! **file** and not a folder, it is appended rather than truncated, every line is
//! flushed, an unset (or empty) variable formats nothing at all, and **it changes
//! no behaviour**.

use crate::trace::Gate;
pub use crate::trace::{Trace, emit};

static GATE: Gate = Gate::new(
    "BT_WEB_TRACE",
    "# BT_WEB_TRACE_V1 elapsed_ms event field=value…",
);

/// The process's trace, opening it on first ask.
pub fn global() -> Option<&'static Trace> {
    GATE.get()
}

/// [`emit`] against the process's own trace — what every station calls.
pub fn line(message: impl FnOnce() -> String) {
    emit(global(), message);
}

/// One seat, spelled the way every station spells it.
///
/// A free function rather than a field on the message, because the two stations
/// that live inside COM callbacks hold the seat by copy and the rest hold it on
/// `self`, and a line that spelled it twice would drift.
pub fn seat(page: bt_platform::PageVisual) -> String {
    format!("tab={} seat={}", page.tab, page.seat)
}

/// A mint as one field: which arm, and the target it stands for.
///
/// `Mint`'s own `Debug` would print the whole `File(String)`, which is the same
/// URL the line already carries in `url=` — this says the *kind* and lets the
/// two be compared at a glance, which is the entire question when a viewer asks
/// for something a shade different from what was minted.
pub fn mint(mint: &crate::webnav::Mint) -> String {
    match mint {
        crate::webnav::Mint::Nothing => String::from("nothing"),
        crate::webnav::Mint::Blank => String::from("blank"),
        crate::webnav::Mint::File(url) => format!("file:{url}"),
        // **Both halves**, because a player is the one mint whose target and
        // whose subject are different files, and a trace that printed only the
        // shell would be a line nobody could tell from an ordinary local page.
        crate::webnav::Mint::VideoShell { url, video } => {
            format!("play:{url} of:{}", video.display())
        }
    }
}

/// A verdict as one field, with the refused reason kept.
pub fn verdict(decision: &crate::webnav::Decision) -> String {
    match decision {
        crate::webnav::Decision::Navigate(target) => format!("navigate:{target}"),
        crate::webnav::Decision::Search(_) => String::from("search"),
        crate::webnav::Decision::Refuse(why) => format!("refuse:{why:?}"),
    }
}
