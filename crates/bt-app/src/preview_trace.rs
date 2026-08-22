//! **`BT_PREVIEW_TRACE` — one named file, one line per preview body that was
//! built and one per frame that drew one differently than the last.**
//!
//! Born of a report this apparatus could not answer without it (2026-08-21): a
//! real markdown file opened into a preview pane drew **its heading rules and
//! none of its words**, and one scroll of the wheel fixed it for good. Every
//! surface around the body was right — the head named the file, the foot printed
//! its path — so the whole of the question is *which* of four things happened to
//! the document between the pool and the glass, and from outside they all look
//! like an empty pane.
//!
//! The four are named on [`bt_render::PreviewTextFrame`], and the two stations
//! here are the two halves of the answer:
//!
//! * `build seat=<n> scale=<f> body=[l,t,r,b] view=<view> blocks=<n> scroll=[x,y]`
//!   — the inputs one body was built from, written **before** it is built, so a
//!   body built at a scale of zero or into a rectangle the layout had not solved
//!   yet says so in its own line rather than being inferred from the picture.
//! * `built seat=<n> paragraphs=<n> quads=<n> blocks=<n>`, or
//!   `built seat=<n> leave=<no-rect|no-buffer|picture>` — what came out.
//! * `frame bodies=<n> paragraphs=<n> quads=<n> drawn=<n> prepared=<0|1>` — what
//!   the renderer then did with all of them.
//!
//! **The frame station writes on a change and never otherwise**, which is
//! [`crate::attention_trace`]'s rule and for its reason exactly: a preview pane
//! standing still is sixty identical frames a second, and a file that wrote all
//! of them would bury the one frame the reader is looking for. The build station
//! is already event-driven — a body is rebuilt when something about it changed —
//! so it writes every time it runs.
//!
//! **It changes no behaviour.** Same shape as [`crate::mouse_trace`] and
//! [`crate::attention_trace`]: the value is a *file* and not a folder, it is
//! appended rather than truncated, every line is flushed, and an unset variable
//! formats nothing at all.

use crate::trace::Gate;
pub use crate::trace::{Trace, emit};

static GATE: Gate = Gate::new(
    "BT_PREVIEW_TRACE",
    "# BT_PREVIEW_TRACE_V1 elapsed_ms event field=value…",
);

/// The process's trace, opening it on first ask.
pub fn global() -> Option<&'static Trace> {
    GATE.get()
}

/// The frame station's memory — the last line it wrote, so that it writes only
/// when the answer moves.
///
/// A field on the window rather than a `static`, because two windows draw two
/// sets of documents and a shared one would make each window's stillness look
/// like the other window's change.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameEcho(Option<bt_render::PreviewTextFrame>);

impl FrameEcho {
    /// Report this frame if it says something the last one did not.
    pub fn changed(&mut self, frame: bt_render::PreviewTextFrame) -> bool {
        if self.0 == Some(frame) {
            return false;
        }
        self.0 = Some(frame);
        true
    }
}

/// `frame …` — what the renderer did with every preview body it holds.
pub fn frame(trace: Option<&Trace>, echo: &mut FrameEcho, frame: bt_render::PreviewTextFrame) {
    if trace.is_none() || !echo.changed(frame) {
        return;
    }
    emit(trace, || {
        format!(
            "frame bodies={} paragraphs={} quads={} drawn={} prepared={}",
            frame.bodies,
            frame.paragraphs,
            frame.quads,
            frame.drawn,
            u8::from(frame.prepared)
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(drawn: usize) -> bt_render::PreviewTextFrame {
        bt_render::PreviewTextFrame {
            bodies: 1,
            paragraphs: 8,
            quads: 2,
            drawn,
            prepared: true,
        }
    }

    /// **A pane standing still writes one line, not sixty a second.**
    ///
    /// The rule `the_attention_trace_writes_one_line_per_decision_and_none
    /// _otherwise` pins one trace over, said about the surface that is redrawn
    /// most often in this window.
    #[test]
    fn a_frame_that_says_nothing_new_writes_nothing() {
        let mut echo = FrameEcho::default();
        assert!(echo.changed(sample(8)), "the first frame is always news");
        assert!(!echo.changed(sample(8)));
        assert!(!echo.changed(sample(8)));
        assert!(
            echo.changed(sample(0)),
            "and the frame that stopped drawing its words is exactly the news"
        );
        assert!(!echo.changed(sample(0)));
    }
}
