//! **`BT_GLYPH_CENSUS` — one named file, one line each time a frame's demand on
//! the glyph atlas changes.**
//!
//! §7.1.3l diagnosed a 4K all-Chinese window that lost a whole rendered page's
//! words from a screenshot and a one-line log, and wrote down that the number
//! behind it — how many distinct glyph bitmaps one frame asks for, against how
//! many the device will hold — had been measured once, by hand, and could not be
//! measured again. §7.1.3m's fixture measures it headless; this measures it on
//! the machine that filed the report.
//!
//! The line is [`bt_render::GlyphCensus::line`] and its fields are that type's
//! own: instances asked for, distinct rasters, distinct faces-and-sizes with the
//! subpixel bins collapsed, rasters two lanes share, the area they cover, the
//! device's roof, and the ratio between them.
//!
//! **The census is expensive and it is off.** Counting a raster's size means
//! rasterizing it a second time, so a frame under this is materially slower than
//! the frame it measures — which is exactly right for an instrument and exactly
//! wrong for a default. Nothing is switched on unless this variable names a
//! file.
//!
//! **It writes on a change and never otherwise**, which is
//! [`crate::preview_trace`]'s rule and for its reason: a window standing still
//! is sixty identical frames a second.
//!
//! **It changes no behaviour.**

use crate::trace::Gate;
pub use crate::trace::{Trace, emit};

static GATE: Gate = Gate::new(
    "BT_GLYPH_CENSUS",
    "# BT_GLYPH_CENSUS_V1 elapsed_ms event field=value…",
);

/// The process's trace, opening it on first ask.
pub fn global() -> Option<&'static Trace> {
    GATE.get()
}

/// The station's memory — the last line it wrote, so it writes only when the
/// answer moves.
///
/// The whole line and not the numbers behind it, because the question this
/// answers is "did the frame's demand change" and the line is exactly that
/// question's answer written down. A field on the window for
/// [`crate::preview_trace::FrameEcho`]'s reason: two windows draw two sets of
/// text and a shared memory would make one window's stillness look like the
/// other's change.
#[derive(Clone, Debug, Default)]
pub struct CensusEcho(Option<String>);

impl CensusEcho {
    /// Report this line if it says something the last one did not.
    fn changed(&mut self, line: &str) -> bool {
        if self.0.as_deref() == Some(line) {
            return false;
        }
        self.0 = Some(line.to_owned());
        true
    }
}

/// Whether this process wants every frame counted.
pub fn wanted() -> bool {
    global().is_some()
}

/// `BT_GLYPH_CENSUS …` — what this frame asked the atlas for.
pub fn frame(
    trace: Option<&Trace>,
    echo: &mut CensusEcho,
    census: Option<&bt_render::GlyphCensus>,
) {
    let (Some(_), Some(census)) = (trace, census) else {
        return;
    };
    let line = census.line();
    if !echo.changed(&line) {
        return;
    }
    emit(trace, || line);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A window standing still writes one line, not sixty a second.**
    #[test]
    fn a_frame_that_asks_for_the_same_thing_writes_nothing() {
        let mut echo = CensusEcho::default();
        assert!(
            echo.changed("requested=10"),
            "the first frame is always news"
        );
        assert!(!echo.changed("requested=10"));
        assert!(
            echo.changed("requested=40000"),
            "and the frame that suddenly wants four times as much is exactly the news"
        );
        assert!(!echo.changed("requested=40000"));
    }

    /// **No file named, nothing counted, nothing written.**
    #[test]
    fn an_unset_variable_writes_nothing_and_asks_for_nothing() {
        let mut echo = CensusEcho::default();
        frame(None, &mut echo, None);
        assert!(echo.0.is_none());
    }
}
