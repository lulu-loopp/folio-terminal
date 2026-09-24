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
//! * `build seat=<n> scale=<f> body=[l,t,r,b] scroll=[x,y] bytes=<n> owed=<0|1>`
//!   — the inputs one body was built from, written **before** it is built, so a
//!   body built at a scale of zero or into a rectangle the layout had not solved
//!   yet says so in its own line rather than being inferred from the picture.
//!   `bytes` and `owed` are the buffer's own two facts, added on 2026-08-23
//!   because `built … paragraphs=0` had been saying two different things with one
//!   number: a document that really is empty, and a head read whose answer never
//!   came home (it had been taken off the one shared worker channel by another
//!   window and dropped — see `docs/DESIGN.md` §2.4).
//! * `built seat=<n> paragraphs=<n> quads=<n> blocks=<n>`, or
//!   `built seat=<n> leave=<no-rect|no-buffer|picture>` — what came out.
//! * `frame bodies=<n> paragraphs=<n> quads=<n> drawn=<n> prepared=<0|1>` — what
//!   the renderer then did with all of them.
//! * `document bytes=<n> blocks=<n> source=<index|none> parse_us=<n>
//!   intrinsic_us=<n> layout_us=<n> total_us=<n> hits=<n> misses=<n>` — what one
//!   markdown document cost to build, split three ways (ticket T4). This is the
//!   station a keystroke is measured with: see [`DocumentBuild`].
//! * `reflow bytes=<n> blocks=<n> why=<source+width+art+font|frame>
//!   source=<index|none>-><index|none> set=<n>-><n> face=<mono|prose|mixed|none>
//!   realized=<n> math_us=<n> pictures_us=<n> reconcile_us=<n> realize_us=<n>
//!   total_us=<n>` — the same document laid out again with no re-parse
//!   (2026-09-23): what asked for it, the first block drawn as source before
//!   and after, how many blocks were drawn as source before and after (a
//!   selection draws every block it touches), the face they wear, how many
//!   blocks were measured, and where the time went. This is the station
//!   a caret moving into another block — a table flipping to its source — is
//!   measured with; before it the arm wrote nothing. See [`ReflowBuild`].
//! * `math formulas=<n> drawn=<n> asked=<n> worker=<0|1>` and
//!   `math answered set=<0|1> mode=<Display|Inline> em_milli=<n> chars=<n>` —
//!   **why a page is standing on its source text** (M2-7, §13.40). A formula
//!   that has not been typeset yet and one the engine refused draw the identical
//!   thing, which is the author's own LaTeX, and that is the right behaviour and
//!   an unreadable one to diagnose: the reading sweep photographed a Mac window
//!   printing `\int_0^1 x\,dx` where the integral belonged and no instrument in
//!   this workspace could say whether the picture was late, refused, or never
//!   asked for. `formulas` is how many the page has, `drawn` how many it had
//!   pictures for, `asked` how many questions this pass sent, and `worker`
//!   whether the thread that answers them is still running. The second line is
//!   one answer coming back.
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

/// **What one markdown document cost to build** — the third station, and the
/// one a keystroke is measured with (ticket T4; `docs/DESIGN.md` §7.1.3q).
///
/// Research open question 5 asks whether the whole-document re-parse survives a
/// keystroke and answers "measure first": 64 KiB through a hand-written line
/// parser is probably far inside budget, and the expensive halves were expected
/// to be the intrinsics and the fence highlighting — which are now keyed per
/// block content ([`crate::MarkdownIntrinsicKey`]) rather than thrown away on
/// every edit. This is what says so on a real document rather than in an
/// argument: three durations, and the two cache counters that explain the middle
/// one.
///
/// **The clock only runs when the trace is open.** The three `Instant`s are
/// taken inside `preview_trace::global().map(...)`, so a window with the
/// variable unset does not time anything at all — the same discipline the two
/// stations above keep, one level down.
#[derive(Clone, Copy, Debug)]
pub struct DocumentBuild {
    pub bytes: usize,
    pub blocks: usize,
    /// Which block was drawn as source, if any — §7.1.3q's own rule, in the
    /// line that shows what it cost.
    pub source: Option<usize>,
    /// [`crate::MarkdownIntrinsicCache`]'s two counters, since the window
    /// opened: what an edit to one block did *not* have to measure again is the
    /// difference between two of these lines.
    pub hits: u64,
    pub misses: u64,
    pub parse: std::time::Duration,
    pub intrinsic: std::time::Duration,
    pub layout: std::time::Duration,
}

/// `document bytes=<n> blocks=<n> source=<index|none> parse_us=<n>
/// intrinsic_us=<n> layout_us=<n> total_us=<n> hits=<n> misses=<n>`
pub fn document(trace: Option<&Trace>, build: DocumentBuild) {
    emit(trace, || {
        let total = build.parse + build.intrinsic + build.layout;
        format!(
            "document bytes={} blocks={} source={} parse_us={} intrinsic_us={} \
             layout_us={} total_us={} hits={} misses={}",
            build.bytes,
            build.blocks,
            build
                .source
                .map_or_else(|| "none".to_owned(), |index| index.to_string()),
            build.parse.as_micros(),
            build.intrinsic.as_micros(),
            build.layout.as_micros(),
            total.as_micros(),
            build.hits,
            build.misses,
        )
    });
}

/// **Which part of a document's key moved**, when the parse did not — the
/// `why=` of a `reflow` line.
///
/// Four names for the four halves of the key a re-flow can come from, and
/// `frame` when none of them moved and the wrap frame alone did (a scale or a
/// font environment the key had not caught up with yet).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReflowCause {
    /// Another block is drawn as source — the caret crossed into it.
    pub source: bool,
    /// The pane's width or the window's scale.
    pub width: bool,
    /// A formula or a picture landed, the ink or the theme changed, or the
    /// reader scrolled into another band of pictures.
    pub art: bool,
    /// The installed fonts changed.
    pub font: bool,
}

impl ReflowCause {
    fn names(self) -> String {
        let names: Vec<&str> = [
            (self.source, "source"),
            (self.width, "width"),
            (self.art, "art"),
            (self.font, "font"),
        ]
        .into_iter()
        .filter_map(|(moved, name)| moved.then_some(name))
        .collect();
        if names.is_empty() {
            "frame".to_owned()
        } else {
            names.join("+")
        }
    }
}

/// The face the blocks now drawn as source wear, for a `reflow` line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFace {
    /// A fence, a table, a formula, a rule or a picture: the monospace fold.
    Mono,
    /// A heading, a paragraph, a list or a quote: the body face.
    Prose,
    /// Some of each — a selection across a paragraph and a fence.
    Mixed,
}

impl SourceFace {
    /// The face a set of source blocks wears, from each block's own; `None`
    /// for an empty set.
    pub fn of(faces: impl IntoIterator<Item = Self>) -> Option<Self> {
        faces
            .into_iter()
            .reduce(|one, two| if one == two { one } else { Self::Mixed })
    }
}

/// **What one re-flow cost** — [`DocumentBuild`]'s opposite number, for the
/// arm that does not parse (2026-09-23).
///
/// Born of a report this file could not answer: a table under the caret
/// flipped to its source and the window hitched, and the only line in the log
/// was a 573 ms self-report whose thread had burned 31 ms of CPU. A seat change
/// takes the no-parse arm, and that arm wrote nothing here, so whether the
/// window's own work or something it waited on was the cost could not be said.
/// This is the line that says it: the four passes the arm runs over the whole
/// document, each timed, and how many blocks it measured.
///
/// **The clock only runs when the trace is open**, [`DocumentBuild`]'s rule.
#[derive(Clone, Copy, Debug)]
pub struct ReflowBuild {
    pub bytes: usize,
    pub blocks: usize,
    pub cause: ReflowCause,
    /// The first block drawn as source before this re-flow, and after it.
    pub source: (Option<usize>, Option<usize>),
    /// How many blocks were drawn as source before this re-flow, and after it
    /// — one for a caret, every block a selection touches (2026-09-23).
    pub set: (usize, usize),
    pub face: Option<SourceFace>,
    /// Blocks measured by this re-flow — two for a caret crossing from one
    /// visible block into another, the band for a resize.
    pub realized: usize,
    pub math: std::time::Duration,
    pub pictures: std::time::Duration,
    pub reconcile: std::time::Duration,
    pub realize: std::time::Duration,
}

/// `reflow bytes=<n> blocks=<n> why=<…> source=<a>-><b> set=<n>-><n>
/// face=<mono|prose|mixed|none> realized=<n> math_us=<n> pictures_us=<n>
/// reconcile_us=<n> realize_us=<n> total_us=<n>`
pub fn reflow(trace: Option<&Trace>, build: ReflowBuild) {
    emit(trace, || {
        let index = |at: Option<usize>| at.map_or_else(|| "none".to_owned(), |at| at.to_string());
        let total = build.math + build.pictures + build.reconcile + build.realize;
        format!(
            "reflow bytes={} blocks={} why={} source={}->{} set={}->{} face={} realized={} \
             math_us={} pictures_us={} reconcile_us={} realize_us={} total_us={}",
            build.bytes,
            build.blocks,
            build.cause.names(),
            index(build.source.0),
            index(build.source.1),
            build.set.0,
            build.set.1,
            match build.face {
                Some(SourceFace::Mono) => "mono",
                Some(SourceFace::Prose) => "prose",
                Some(SourceFace::Mixed) => "mixed",
                None => "none",
            },
            build.realized,
            build.math.as_micros(),
            build.pictures.as_micros(),
            build.reconcile.as_micros(),
            build.realize.as_micros(),
            total.as_micros(),
        )
    });
}

/// `frame …` — what the renderer did with every preview body it holds.
pub fn frame(trace: Option<&Trace>, echo: &mut FrameEcho, frame: bt_render::PreviewTextFrame) {
    if trace.is_none() || !echo.changed(frame) {
        return;
    }
    emit(trace, || {
        format!(
            "frame bodies={} paragraphs={} quads={} drawn={} prepared={} \
             layers={} layer_bodies={} layer_labels={} layer_paragraphs={} \
             layer_drawn={} layer_prepared={} refused={}",
            frame.bodies,
            frame.paragraphs,
            frame.quads,
            frame.drawn,
            u8::from(frame.prepared),
            frame.layers,
            frame.layer_bodies,
            frame.layer_labels,
            frame.layer_paragraphs,
            frame.layer_drawn,
            u8::from(frame.layer_prepared),
            frame.refused.names()
        )
    });
}

/// `picture read=<file>` — **one line per decode this window asks the disk for
/// on behalf of a page** (`docs/DESIGN.md` §7.1.3u).
///
/// The station the two picture freezes were diagnosed and closed with, and it
/// is here rather than in a scratch counter because the pair of numbers it
/// completes is the pair this whole class of report needs: `document` above says
/// how many times a page was built, and this says how many reads that building
/// sent out. A page that has settled writes neither line; a page in the livelock
/// writes hundreds of both a second, which is what says the two apart from the
/// outside.
///
/// Written at the door ([`crate::Runtime::request_peek_pixels`]'s caller) rather
/// than where the need is noticed, so what it counts is what actually went to
/// the worker.
pub fn picture_read(trace: Option<&Trace>, file: &std::path::Path) {
    emit(trace, || format!("picture read={}", file.display()));
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
            ..bt_render::PreviewTextFrame::default()
        }
    }

    /// RED (preview report 2026-09-23, C1) — **a re-flow's line says what
    /// flipped, what asked for it, how many blocks it measured and where the
    /// time went.**
    ///
    /// Written to a real trace file, in the station format every other line of
    /// `BT_PREVIEW_TRACE` keeps: a millisecond stamp, the event, and
    /// `field=value` pairs. The report this is for is a table flipping into
    /// source, which is `why=source`, a `source=a->b` naming both blocks, and a
    /// face; and the four durations add up to the total.
    ///
    /// MUTATION: drop a field from the format, or leave `names` answering
    /// `frame` for a cause that moved, and the comparison goes red.
    #[test]
    fn a_reflow_line_names_the_flip_and_what_it_cost() {
        let path = std::env::temp_dir().join(format!(
            "bt-preview-trace-{}-reflow.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let trace = Trace::create(&path, "# BT_PREVIEW_TRACE_V1 elapsed_ms event field=value…");
        let micros = std::time::Duration::from_micros;
        reflow(
            Some(&trace),
            ReflowBuild {
                bytes: 53_000,
                blocks: 412,
                cause: ReflowCause {
                    source: true,
                    ..ReflowCause::default()
                },
                source: (Some(96), Some(101)),
                set: (1, 1),
                face: Some(SourceFace::Mono),
                realized: 2,
                math: micros(1_200),
                pictures: micros(300),
                reconcile: micros(2_500),
                realize: micros(4_000),
            },
        );
        reflow(
            Some(&trace),
            ReflowBuild {
                bytes: 10,
                blocks: 1,
                cause: ReflowCause::default(),
                source: (None, None),
                set: (0, 0),
                face: None,
                realized: 0,
                math: micros(0),
                pictures: micros(0),
                reconcile: micros(0),
                realize: micros(0),
            },
        );
        // **A selection reaching from a paragraph into a fence** (owner's ruling
        // 2026-09-23): the line says how many blocks are source before and
        // after, and that they wear both faces.
        reflow(
            Some(&trace),
            ReflowBuild {
                bytes: 53_000,
                blocks: 412,
                cause: ReflowCause {
                    source: true,
                    ..ReflowCause::default()
                },
                source: (Some(96), Some(96)),
                set: (1, 5),
                face: SourceFace::of([SourceFace::Prose, SourceFace::Mono, SourceFace::Prose]),
                realized: 4,
                math: micros(0),
                pictures: micros(0),
                reconcile: micros(0),
                realize: micros(0),
            },
        );
        let written = std::fs::read_to_string(&path).expect("the trace file was created");
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(
            lines.len(),
            4,
            "a header and one line per re-flow: {written:?}"
        );
        assert!(
            lines[1].ends_with(
                "reflow bytes=53000 blocks=412 why=source source=96->101 set=1->1 face=mono realized=2 \
                 math_us=1200 pictures_us=300 reconcile_us=2500 realize_us=4000 total_us=8000"
            ),
            "{:?}",
            lines[1],
        );
        assert!(
            lines[2].ends_with(
                "why=frame source=none->none set=0->0 face=none realized=0 \
                 math_us=0 pictures_us=0 reconcile_us=0 realize_us=0 total_us=0"
            ),
            "{:?}",
            lines[2],
        );
        assert!(
            lines[3].contains("source=96->96 set=1->5 face=mixed realized=4 "),
            "{:?}",
            lines[3],
        );
        assert_eq!(
            SourceFace::of([SourceFace::Prose, SourceFace::Prose]),
            Some(SourceFace::Prose)
        );
        assert_eq!(SourceFace::of([]), None);
        let _ = std::fs::remove_file(&path);
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
