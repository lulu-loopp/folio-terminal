//! **`BT_CARD_TRACE` — one named file, one line per station on a focus card's
//! road** (T-CARD-TRACE; §7.1.6b′).
//!
//! A focus card is a miniature of a pane's transcript, and where in that
//! transcript it is standing is decided by three things that never meet: the
//! per-frame pass ([`crate::focus_thumb::FocusThumbnails::trace_card_walk`]),
//! the wheel's aim ([`crate::focus_thumb::aim_card_skip`]), and — underneath
//! both — the *grid the pane is wearing*, which a resize or a display of another
//! scale rewrites without the card being asked. T-CARD-ANCHOR-DPI answered one
//! report about that; the owner's next one — a card that shows a different
//! window of the transcript after a 4K display at 200% and a 1080p display at
//! 150% have each had the window once, and never comes back — could not be
//! reproduced from deterministic inputs by two audits. So the owner will record
//! a real session, and the card has to write down what it does.
//!
//! **Two clamps, and only one of them writes.** A card's position is a plain
//! number of rows above the tail (T-CARD-RESTORE-NEXT59), and it is bounded in
//! two separate places: a wheel notch clamps it on the way in, so no debt
//! survives past the top, and [`crate::focus_thumb::transcript_tail`] clamps it
//! again at draw time and writes nothing back — so the number a session file
//! carries may legitimately be larger than the row a reader is looking at. The
//! per-frame pass held it down too, until this recording showed what that costs
//! a reader whose window is crossing to a display of another scale
//! (T-CARD-NO-PASSIVE-CLAMP); it now reports and writes nothing. Telling the
//! stored number and the drawn one apart from the outside is impossible, and
//! telling them apart is the whole of this file: `skip_before`/`skip_after`/
//! `drawn` on one station, `stored_before`/`clamped_before` on the other.
//!
//! **This is forensic apparatus and nothing else: it changes no behaviour.**
//! The idiom is [`BT_MOUSE_TRACE`](crate::mouse_trace)'s, down to the last rule
//! — the value is a *file* and never a folder, set-but-empty is off, the file is
//! appended rather than truncated, every line is flushed as it is written, and
//! an unset variable formats nothing at all because every station is handed a
//! closure. The machinery is [`crate::trace`]'s; this module is the variable's
//! name, its header, and one builder per station.
//!
//! **A traced run is slower than the run it measures.** `card walk` takes a
//! bounded transcript walk of its own so that it can report the reachable
//! maximum and the two rows a reader sees, which the draw it is watching does
//! not hand back. The walk is bounded exactly as the draw's is and it happens
//! only when this variable names a file — [`crate::glyph_trace`]'s bargain, said
//! about a different instrument.
//!
//! **The clock is the process's and not this file's** ([`crate::trace::Trace`]),
//! which is what makes a `BT_CARD_TRACE` and a `BT_MOUSE_TRACE` of the same run
//! mergeable by their first column: the gesture that moves a card is a wheel
//! notch, and the wheel's own road is written in the other file.
//!
//! # The stations
//!
//! * `card walk` — every call of the per-frame pass, whichever of its five
//!   exits it took, with the grid the pane is wearing, the rows the card holds,
//!   the stored offset on both sides (one number, since the pass writes
//!   nothing), how far the walk could reach, the offset the *draw* will use, and
//!   the text of the card's first and last row.
//! * `card aim` — every wheel aim: the detents, the stored offset, what the
//!   entry clamp made of it, what was asked for and what was given.
//! * `card pane resized` — every new grid a pane behind a card is given.
//! * `card scale` — the scale road, with the rows each card holds at the new one.
//!
//! Every builder below is a pure function of what its station read, so the
//! format is a thing a test can hold rather than a format string scattered over
//! two files.

use bt_layout::SeatId;

use crate::TabId;
use crate::trace::Gate;

pub use crate::trace::{Trace, emit};

/// The variable that names the file, and the first line of every opened trace so
/// that a file which has collected several runs can still be told what it is and
/// where each run began.
static GATE: Gate = Gate::new(
    "BT_CARD_TRACE",
    "# BT_CARD_TRACE_V1 elapsed_ms event field=value…",
);

/// The process's trace, opening it on first ask.
pub fn global() -> Option<&'static Trace> {
    GATE.get()
}

/// Whether anything is listening — for the two stations that must *compute*
/// their fields (a transcript walk; a solve of the card column) rather than
/// merely format them. [`crate::mouse_trace::is_on`]'s reason exactly.
pub fn is_on() -> bool {
    global().is_some()
}

/// [`emit`] against the process's own trace — what every station calls.
pub fn line(message: impl FnOnce() -> String) {
    emit(global(), message);
}

/// **Which card a line is about, and why the station ran.**
///
/// The window is a `u64` read before any borrow, for
/// [`crate::mouse_trace::window_line`]'s reason: the stations below stand inside
/// a live `&mut` of a leaf's own fields, where `&self` cannot be taken. The tab
/// and the seat are the pair that names a pane for the life of the process —
/// `LeafId`'s own two halves, spelled apart because `LeafId`'s `Debug` has braces
/// and blanks in it, and a value with a blank in it is not a value on a
/// `key=value` line.
///
/// **`why` is the caller's and never guessed.** The per-frame pass is reached
/// from a frame and from the notch that re-spends the projection, and which of
/// the two it was is the first thing a reader of a card's road needs; nothing
/// inside the pass can answer it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Card {
    pub window: u64,
    pub tab: TabId,
    pub seat: SeatId,
    pub why: &'static str,
}

impl Card {
    /// The four fields every station but the resize opens with.
    #[must_use]
    pub fn words(&self) -> String {
        format!(
            "window={} tab={:?} seat={:?} why={}",
            self.window, self.tab, self.seat, self.why
        )
    }

    /// **A card nobody is watching** — the identity a pure test hands the clamp
    /// and the aim, which require one and never read it while the variable is
    /// unset.
    #[cfg(test)]
    #[must_use]
    pub fn untraced() -> Self {
        Self {
            window: 0,
            tab: TabId(0),
            seat: SeatId(0),
            why: why::FRAME,
        }
    }
}

/// **The reasons a station can carry**, declared here rather than spelled at the
/// stations, so the vocabulary is closed and a test can hold it closed.
///
/// A word nobody declared is a road nobody thought about, which is the shape of
/// the silence this file exists to end.
pub mod why {
    /// The per-frame projection pass.
    pub const FRAME: &str = "frame";
    /// `Alt`+wheel over a card.
    pub const WHEEL: &str = "wheel";
    /// A new scale arrived and was applied.
    pub const SCALE_APPLIED: &str = "scale-applied";
    /// The rectangle a scale change was owed arrived.
    pub const RECTANGLE_SETTLED: &str = "rectangle-settled";
}

/// How many characters of a card row reach the file.
///
/// A mini seat is a dozen-odd columns wide at the sizes this defect was reported
/// at, so forty is the whole row and a little more — while a pane's own line,
/// which a card row is cut from and which can be thousands of columns, is not
/// something this file is entitled to put on somebody's disk.
pub const ROW_CHARACTERS: usize = 40;

/// **One card row as one quoted, escaped, bounded field.**
///
/// Quoted because a row has blanks in it and a blank is a field separator here;
/// escaped because a row can carry anything a program printed, and a raw control
/// character in a log file is a log file that lies about how many lines it has.
/// Cut at [`ROW_CHARACTERS`] *characters* and not bytes, because a byte cut
/// through a multi-byte character is not text.
#[must_use]
pub fn row_word(text: Option<&str>) -> String {
    let Some(text) = text else {
        return "-".to_owned();
    };
    let mut out = String::with_capacity(ROW_CHARACTERS + 2);
    out.push('"');
    let mut cut = false;
    for (index, character) in text.chars().enumerate() {
        if index == ROW_CHARACTERS {
            cut = true;
            break;
        }
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control.is_control() => {
                out.push_str(&format!("\\x{:02x}", control as u32));
            }
            plain => out.push(plain),
        }
    }
    if cut {
        out.push('…');
    }
    out.push('"');
    out
}

/// A count a station may not have reached, as one field's value.
fn count_word(count: Option<usize>) -> String {
    count.map_or_else(|| "-".to_owned(), |count| count.to_string())
}

/// **The per-frame pass, whatever it found** — the station a report of "the
/// card is showing the wrong window" is read off.
///
/// One builder and one line for all five of the pass's exits, because what a
/// reader needs is the same fields whichever exit was taken and `leave=` is the
/// one word that says which. A field a refusing exit never computed says so with
/// `-`; an absent number is not a zero, and printing one would invent a walk that
/// never happened.
///
/// **`drawn` is the field this station exists for.** The pass writes nothing back
/// into the leaf (T-CARD-NO-PASSIVE-CLAMP), and
/// [`crate::focus_thumb::transcript_tail`] clamps at draw time and writes nothing
/// back either. So `skip_before` and `skip_after` are the number the session file
/// will carry — one number, on every exit — and `drawn` is the row a reader is
/// actually looking at, which is smaller whenever the grid the pane is wearing
/// cannot reach that far. Nothing else in the program compares them.
pub struct Walk {
    pub card: Card,
    /// Which exit the pass took — one of `WALK_EXITS`.
    pub leave: &'static str,
    /// The grid the pane is wearing, in cells.
    pub grid: (u32, u32),
    /// How many rows the card holds.
    pub rows: usize,
    pub skip_before: usize,
    pub skip_after: usize,
    /// The largest offset this bounded walk could reach — `climb.len() - rows`.
    pub reachable: Option<usize>,
    /// What the draw will use: `skip_after.min(reachable)`.
    pub drawn: Option<usize>,
    pub first: Option<String>,
    pub last: Option<String>,
}

impl Walk {
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "card walk {} leave={} grid={}x{} rows={} skip_before={} skip_after={} max={} \
             drawn={} first={} last={}",
            self.card.words(),
            self.leave,
            self.grid.0,
            self.grid.1,
            self.rows,
            self.skip_before,
            self.skip_after,
            count_word(self.reachable),
            count_word(self.drawn),
            row_word(self.first.as_deref()),
            row_word(self.last.as_deref()),
        )
    }
}

/// **Every way out of the per-frame pass**, declared once so the pass's own
/// literals can be checked against a list.
///
/// `cfg(test)` for [`crate::mouse_trace::WHEEL_ROUTES`]'s reason: the pass
/// writes its word beside the decision it describes, and a copy compiled into
/// the product would be a second list for somebody to forget.
///
/// **`unchanged` is not "the card did not move".** The damage key a card is
/// compared on carries its own `skip`, so a card the wheel has just moved always
/// looks damaged and never takes that exit — which is right, and worth saying
/// because the word on its own reads the other way.
#[cfg(test)]
pub const WALK_EXITS: [&str; 5] = [
    // The card is resting on the newest line; there is nothing above it to
    // report on.
    "at-tail",
    // The picture this card is showing is the picture this demand asks for.
    "unchanged",
    // Inside the projection clock, with no gesture credit to jump it.
    "throttled",
    // The seat is not a terminal, so there is no transcript to be above.
    "not-a-terminal",
    // The walk ran: the stored offset stands, and `drawn` says what the draw
    // will make of it.
    "walked",
];

/// **What one `Alt`+wheel aim did to the card's window.**
///
/// `clamped_before` is the field the restored model exists for: the aim clamps
/// the stored offset **on the way in**, so a card left pointing past the top by a
/// resize is brought back to the reachable maximum before the detents are
/// counted, and the very first notch in the other direction moves. A line where
/// `stored_before` and `clamped_before` differ is that repair happening.
///
/// `requested` beside `skip_after` is the whole of "clamp applied" at the other
/// end: a card driven past the top of what the transcript holds asks for one
/// number and is given another.
pub struct Aim {
    pub card: Card,
    pub detents: i32,
    pub rows: usize,
    /// The leaf's number as the notch found it.
    pub stored_before: usize,
    /// The same number after the aim's entry clamp — where the detents count from.
    pub clamped_before: usize,
    pub requested: usize,
    pub skip_after: usize,
    /// The largest offset this bounded walk could reach — `climb.len() - rows`.
    pub reachable: usize,
}

impl Aim {
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "card aim {} detents={} rows={} stored_before={} clamped_before={} requested={} \
             skip_after={} max={} clamped={}",
            self.card.words(),
            self.detents,
            self.rows,
            self.stored_before,
            self.clamped_before,
            self.requested,
            self.skip_after,
            self.reachable,
            u8::from(self.requested != self.skip_after),
        )
    }
}

/// **A pane behind a card given a new grid** — the event underneath every card
/// report this file exists for, because a grid change is not a view: it re-wraps
/// the live screen and freezes whatever it pushes off the top at the width it
/// pushed it off *at*, for ever (§7.1.6b′, T-CARD-ANCHOR-DPI).
///
/// No `why=` on this one. The schedulers carry a `context` sentence for an error
/// message and a sentence has blanks in it; what a reader of this station needs
/// is the two words below, which are facts about the pane rather than about who
/// asked.
pub struct PaneResized {
    pub pane: Pane,
    /// `shown` or `behind` — whether this pane's tab is the one on the glass.
    pub stage: &'static str,
    /// One of `RESIZE_COMMITS`.
    pub commit: &'static str,
    pub before: (u32, u32),
    pub after: (u32, u32),
}

/// **Which pane a resize line is about** — [`Card`] without the reason, because
/// the grid road has no card of its own to have a reason about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pane {
    pub window: u64,
    pub tab: TabId,
    pub seat: SeatId,
}

impl Pane {
    /// **A pane nobody is watching** — `Card::untraced`'s counterpart, for the
    /// fixtures that drive a resize without a window behind it.
    #[cfg(test)]
    #[must_use]
    pub fn untraced() -> Self {
        Self {
            window: 0,
            tab: TabId(0),
            seat: SeatId(0),
        }
    }
}

impl PaneResized {
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "card pane resized window={} tab={:?} seat={:?} stage={} commit={} \
             grid_before={}x{} grid_after={}x{}",
            self.pane.window,
            self.pane.tab,
            self.pane.seat,
            self.stage,
            self.commit,
            self.before.0,
            self.before.1,
            self.after.0,
            self.after.1,
        )
    }
}

/// **The three things that can happen to a pane's grid**, declared here for
/// `WALK_EXITS`'s reason.
#[cfg(test)]
pub const RESIZE_COMMITS: [&str; 3] = [
    // The pane is on the glass, so its own actor reflowed in this very turn and
    // its child hears the size at the quiet boundary.
    "local",
    // The pane is behind another tab: nothing reflowed, and both the reflow and
    // the notification wait for the quiet boundary.
    "queued",
    // The quiet boundary released it: the deferred reflow, if there was one, and
    // the `ResizePseudoConsole` the child hears.
    "committed",
];

/// **A scale change, measured at the card it moves.**
///
/// The rows are the point. A card's height is not a function of its logical box
/// alone — `focus_thumb::mini_rows` rounds a border, two paddings and a line
/// height each on its own — so how many rows a card holds is a thing only the new
/// scale can answer, and it is the unit every station above is counted in.
pub struct Scale {
    pub card: Card,
    pub before: f64,
    pub after: f64,
    pub rows: usize,
}

impl Scale {
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "card scale {} scale_before={} scale_after={} rows={}",
            self.card.words(),
            self.before,
            self.after,
            self.rows,
        )
    }
}

/// **The same scale change in a window that is drawing no cards.**
///
/// Said out loud rather than passed over in silence: "this window was on the road
/// and had no column" is one of the readings a merged trace has to be able to
/// make, and an absent line cannot be told from a station that never ran.
#[must_use]
pub fn scale_without_a_column(window: u64, why: &'static str, before: f64, after: f64) -> String {
    format!(
        "card scale window={window} why={why} scale_before={before} scale_after={after} \
         column=none"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(why: &'static str) -> Card {
        Card {
            window: 7,
            tab: TabId(3),
            seat: SeatId(2),
            why,
        }
    }

    fn pane() -> Pane {
        Pane {
            window: 7,
            tab: TabId(3),
            seat: SeatId(2),
        }
    }

    /// **Every station is one line a reader can grep**, held to the byte —
    /// `bt_platform::hotkey`'s own gate, for its reason: a trace is read by
    /// somebody who was not here, and the shape of the line is the whole of what
    /// they are promised.
    #[test]
    fn every_station_of_the_trace_is_one_line_a_reader_can_grep() {
        assert_eq!(
            Walk {
                card: card(why::FRAME),
                leave: "walked",
                grid: (240, 24),
                rows: 8,
                skip_before: 130,
                skip_after: 130,
                reachable: Some(116),
                drawn: Some(116),
                first: Some("H001".to_owned()),
                last: Some("PS D:\\Developer> cargo test".to_owned()),
            }
            .line(),
            "card walk window=7 tab=TabId(3) seat=SeatId(2) why=frame leave=walked grid=240x24 \
             rows=8 skip_before=130 skip_after=130 max=116 drawn=116 first=\"H001\" \
             last=\"PS D:\\\\Developer> cargo test\"",
            "a walk reports one stored number on both sides and clamps only `drawn`"
        );
        assert_eq!(
            Walk {
                card: card(why::FRAME),
                leave: "throttled",
                grid: (80, 24),
                rows: 8,
                skip_before: 130,
                skip_after: 130,
                reachable: None,
                drawn: None,
                first: None,
                last: None,
            }
            .line(),
            "card walk window=7 tab=TabId(3) seat=SeatId(2) why=frame leave=throttled \
             grid=80x24 rows=8 skip_before=130 skip_after=130 max=- drawn=- first=- last=-",
            "an exit that walked nothing prints no walk, and `-` is not a zero"
        );
        assert_eq!(
            Aim {
                card: card(why::WHEEL),
                detents: -1,
                rows: 12,
                stored_before: 16,
                clamped_before: 8,
                requested: 7,
                skip_after: 7,
                reachable: 8,
            }
            .line(),
            "card aim window=7 tab=TabId(3) seat=SeatId(2) why=wheel detents=-1 rows=12 \
             stored_before=16 clamped_before=8 requested=7 skip_after=7 max=8 clamped=0",
            "the entry clamp is what makes the first reverse notch move"
        );
        assert_eq!(
            Aim {
                card: card(why::WHEEL),
                detents: 40,
                rows: 4,
                stored_before: 100,
                clamped_before: 100,
                requested: 140,
                skip_after: 116,
                reachable: 116,
            }
            .line(),
            "card aim window=7 tab=TabId(3) seat=SeatId(2) why=wheel detents=40 rows=4 \
             stored_before=100 clamped_before=100 requested=140 skip_after=116 max=116 clamped=1",
            "the top of the transcript is a clamp and the line says so"
        );
        assert_eq!(
            PaneResized {
                pane: pane(),
                stage: "shown",
                commit: "local",
                before: (240, 60),
                after: (320, 60),
            }
            .line(),
            "card pane resized window=7 tab=TabId(3) seat=SeatId(2) stage=shown commit=local \
             grid_before=240x60 grid_after=320x60"
        );
        assert_eq!(
            Scale {
                card: card(why::SCALE_APPLIED),
                before: 2.0,
                after: 1.5,
                rows: 6,
            }
            .line(),
            "card scale window=7 tab=TabId(3) seat=SeatId(2) why=scale-applied scale_before=2 \
             scale_after=1.5 rows=6"
        );
        assert_eq!(
            scale_without_a_column(7, why::RECTANGLE_SETTLED, 1.5, 1.5),
            "card scale window=7 why=rectangle-settled scale_before=1.5 scale_after=1.5 \
             column=none"
        );
    }

    /// **One event is one line, and no field on it is a dump.**
    ///
    /// The two card rows are the only thing on any station that a shell can
    /// grow — which is exactly why they are cut — so the ceiling is asserted on
    /// them first: forty characters, four bytes of escape apiece at worst, and
    /// the cut mark. Everything else on a line is an identifier or a count, and
    /// the whole `card walk` line is checked against a ceiling only a `Debug`
    /// dump somebody added could cross.
    #[test]
    fn no_station_writes_a_second_line_or_a_paragraph() {
        let worst_row = row_word(Some(&"\u{7}".repeat(200)));
        assert!(
            worst_row.chars().count() <= 4 * ROW_CHARACTERS + 3,
            "an escaped row is bounded by its cut: {} characters",
            worst_row.chars().count()
        );
        let realistic = Walk {
            card: card(why::FRAME),
            leave: "walked",
            grid: (320, 90),
            rows: 13,
            skip_before: 98_000,
            skip_after: 98_000,
            reachable: Some(1_240),
            drawn: Some(1_240),
            first: Some("x".repeat(400)),
            last: Some("y".repeat(400)),
        }
        .line();
        assert!(
            !realistic.contains('\n'),
            "one station is one line: {realistic:?}"
        );
        assert!(
            realistic.chars().count() <= 260,
            "a station wrote {} characters, which is a paragraph and not a line: {realistic:?}",
            realistic.chars().count()
        );
        for station in [
            Aim {
                card: card(why::WHEEL),
                detents: 1,
                rows: 13,
                stored_before: 98_000,
                clamped_before: 1_240,
                requested: 1_241,
                skip_after: 1_240,
                reachable: 1_240,
            }
            .line(),
            PaneResized {
                pane: pane(),
                stage: "behind",
                commit: "queued",
                before: (240, 60),
                after: (320, 60),
            }
            .line(),
            Scale {
                card: card(why::SCALE_APPLIED),
                before: 2.0,
                after: 1.5,
                rows: 13,
            }
            .line(),
            scale_without_a_column(7, why::SCALE_APPLIED, 2.0, 1.5),
        ] {
            assert!(
                !station.contains('\n'),
                "one station is one line: {station:?}"
            );
            assert!(
                station.chars().count() <= 200,
                "a station with no card text on it wrote a paragraph: {station:?}"
            );
        }
    }

    /// **A row reaches the file cut, escaped and quoted** — the three properties
    /// that stop a shell's output from rewriting the log it is being recorded
    /// into.
    #[test]
    fn a_card_row_is_cut_escaped_and_quoted() {
        assert_eq!(row_word(None), "-");
        assert_eq!(row_word(Some("")), "\"\"");
        assert_eq!(
            row_word(Some("a\tb\r\nc")),
            "\"a\\tb\\r\\nc\"",
            "a control character is spelled and never written"
        );
        assert_eq!(
            row_word(Some("\u{7}\u{1b}[31m")),
            "\"\\x07\\x1b[31m\"",
            "a bell and an escape are two characters, not a beep and a colour"
        );
        assert_eq!(
            row_word(Some(&"z".repeat(41))),
            format!("\"{}…\"", "z".repeat(ROW_CHARACTERS)),
            "cut at forty characters, and the cut is visible"
        );
        assert_eq!(
            row_word(Some(&"字".repeat(41))),
            format!("\"{}…\"", "字".repeat(ROW_CHARACTERS)),
            "forty characters and never forty bytes"
        );
        assert_eq!(
            row_word(Some(&"z".repeat(ROW_CHARACTERS))),
            format!("\"{}\"", "z".repeat(ROW_CHARACTERS)),
            "a row that fits is not marked as cut"
        );
        assert_eq!(
            row_word(Some(r#"say "hi" \ once"#)),
            r#""say \"hi\" \\ once""#,
            "a quote and a backslash are escaped, so the field stays one field"
        );
    }

    /// **Unset is off, and off writes nothing at all** — not an empty file, no
    /// file. `BT_CARD_TRACE=` is a shell saying "not this run"
    /// ([`crate::trace`]), and a run that answered it with a file named the empty
    /// string would fail in a way that looks like the feature is broken.
    ///
    /// The gate is exercised through [`emit`] rather than through the `static`
    /// above, because setting a process-wide variable is `unsafe` in this edition
    /// and would race every other test in this binary.
    #[test]
    fn an_unset_variable_opens_no_file_and_formats_no_line() {
        let path = std::env::temp_dir().join(format!(
            "bt-card-trace-unset-{}-{:?}.log",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut formatted = false;
        emit(None, || {
            formatted = true;
            let _ = std::fs::write(&path, "this must never be written");
            String::from("card walk")
        });
        assert!(
            !formatted,
            "an unset trace variable must not format its line"
        );
        assert!(
            !path.exists(),
            "an unset trace variable must not create the file it does not name"
        );
    }

    /// **The vocabularies are closed**, and these are the lists the stations'
    /// literals are checked against.
    #[test]
    fn the_words_a_station_may_write_are_written_down_once() {
        for word in WALK_EXITS
            .into_iter()
            .chain(RESIZE_COMMITS)
            .chain([
                why::FRAME,
                why::WHEEL,
                why::SCALE_APPLIED,
                why::RECTANGLE_SETTLED,
            ])
            .chain(["shown", "behind"])
        {
            assert!(
                !word.is_empty() && !word.contains(' ') && !word.contains('='),
                "a word with a blank or an equals in it is not a value on a key=value line: \
                 {word:?}"
            );
        }
    }
}
