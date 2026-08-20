//! **The projection budget behind the focus column's live thumbnails**
//! (`docs/DESIGN.md` §7.1.6b′ F2).
//!
//! §7.1.6b′ files F2 as "a projection cost somebody has to set a budget for —
//! one shrunk projection per seat of every visible tab". This module is that
//! budget, written down in one place so that it can be read, argued with and
//! **tested**, instead of being whatever the painter happened to do.
//!
//! # The four gates, cheapest first
//!
//! 1. **The mode.** Focus off and nothing here runs at all: [`FocusThumbnails`]
//!    is emptied on the way out ([`FocusThumbnails::clear`]) and the frame that
//!    follows costs exactly what a frame cost before F2 existed. This is the gate
//!    that makes "leaving the mode gives the cost back" true by construction
//!    rather than by a cache slowly going cold.
//! 2. **Visibility.** Only a card *inside the column's clip box* is projected.
//!    The caller filters on the same rectangle the painter draws through
//!    (`FocusRailGeometry::viewport`), so "if you can see it, it is live; if you
//!    scrolled past it, it stopped" holds by one number rather than by two rules
//!    agreeing. A card scrolled out has its entry dropped, so scrolling back
//!    projects it fresh rather than flashing whatever it said a minute ago.
//! 3. **Damage.** A seat re-projects only when the thing it is a picture *of*
//!    has changed: a terminal's `screen_revision`, a files column's
//!    [`DirCache::revision`] together with the state that decides which rows are
//!    open, a face's own two words. **This is the gate that makes an idle window
//!    free** — twenty seats with nothing happening in them answer twenty integer
//!    comparisons, produce no allocation, and hand back the same strings, so the
//!    label list the renderer is given is byte-identical and `set_chrome` uploads
//!    nothing.
//! 4. **The clock.** A seat that *is* changing re-projects at most once every
//!    [`MIN_INTERVAL`]. A shell writing at 10 000 lines a second and a shell
//!    writing at ten cost the same, because a card 92 pixels tall cannot show the
//!    difference and the eye cannot see it.
//!
//! The gates are in that order on purpose. Damage is asked *before* the clock
//! because reading a generation is free and refusing on it costs nothing; the
//! clock is asked after, because its job is to bound the *rate* of real work, not
//! to add a second reason to do none.
//!
//! # What one projection is
//!
//! One seat's content, rebuilt: six rows of a terminal's tail cut to that seat's
//! own column count, four rows of a files column's tree, or a face's two words.
//! [`ThumbStats::projections`] counts exactly those, which is what the F2 tests
//! assert on — a counter, not a stopwatch, so the gates are checked for what they
//! are rather than for how fast the machine that ran them was.
//!
//! # Where the width comes in
//!
//! §3.3's multi-projection — the same session presented at more than one width —
//! is the machine floor this stands on, and the shape it takes here is the
//! **cut**: each seat is handed the number of characters that fit across *its*
//! rectangle at the mini font's advance, and a row longer than that is cut before
//! it is ever shaped. That is why `columns` is part of a terminal seat's damage
//! key: a card that changed width has to re-cut even though the grid behind it
//! did not move.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use bt_layout::SeatId;
use bt_term::DualPlaneSession;

use crate::{
    TabId,
    files::{self, DirCache, RowKind},
    seats::{FilesLeafState, MiniFilesRow, MiniSeatContent},
};

/// **The rate ceiling: one projection per seat per 100 ms — 10 Hz.**
///
/// Chosen against the thing being drawn rather than against the display: a mini
/// seat carries six rows of 7.5px text inside a 92px card, and the difference
/// between a tail redrawn ten times a second and one redrawn a hundred times is
/// not visible at that size — it is only payable. Ten is also comfortably above
/// the rate at which a reader can follow a scrolling card at all, so nothing
/// legible is being withheld.
///
/// It bounds the **worst** case and not the common one: gate 3 above means a
/// quiet seat does not project at 10 Hz, it does not project at all.
pub const MIN_INTERVAL: Duration = Duration::from_millis(100);

/// How many rows of a terminal's tail a card carries, and how many rows of a
/// files column's tree — the mock-up's `slice(-6)` and `slice(0, 4)`, read from
/// the one place they are written down.
use bt_render::{FOCUS_MINI_FILES_ROWS, FOCUS_MINI_TERM_ROWS};

/// What the budget did this frame, in counts.
///
/// **Counters and not timings**, because a test that asserts "an idle window does
/// no work" has to be able to say so without owning the machine it runs on: the
/// claim is that nothing was rebuilt, and `projections` staying put is that claim
/// exactly. The three `skipped_*` fields say *which gate* refused, so a
/// regression that quietly moves work from one gate to another cannot hide behind
/// a total.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThumbStats {
    /// Seats whose content was actually rebuilt.
    pub projections: u64,
    /// Seats that had nothing new to say — gate 3.
    pub skipped_unchanged: u64,
    /// Seats that had something new to say too soon after the last time — gate 4.
    pub skipped_throttled: u64,
    /// Cards dropped because they scrolled out of the column — gate 2. Counted in
    /// cards and not in seats, because that is the unit the gate refuses in.
    pub dropped_offscreen: u64,
}

/// What a seat's content was last built from — the question "could this possibly
/// look different now?" in the smallest form that can answer it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Damage {
    /// A terminal seat: the screen's own damage counter, and the width it was cut
    /// to.
    ///
    /// Both, because either can change the six strings on its own — new output
    /// moves the tail, and a narrower card cuts it shorter.
    ///
    /// `screen_revision` and **not** `grid_generation`, which is the near-miss
    /// this branch actually made first: that one counts reflow boundaries, so it
    /// stands perfectly still through a shell printing all day and a card keyed
    /// to it never updates at all. See `DualPlaneSession::screen_revision`.
    Grid { revision: u64, columns: usize },
    /// A files column: the cache's write counter, and the state that decides
    /// which of its rows are visible.
    ///
    /// The state is carried whole rather than hashed. It is a root, an open set
    /// and a selection — small, `Eq`, and already cloned by every other reader of
    /// it — while a hash would be a second answer to "did this change" that can
    /// be wrong in the direction that matters.
    Files {
        revision: u64,
        state: FilesLeafState,
    },
    /// A face: the two words on it. There is nothing behind them to have a
    /// generation, so they *are* the generation.
    Face { name: String, kind: String },
}

/// One seat's entry: what it says, what that was built from, and when.
#[derive(Clone, Debug)]
struct Entry {
    damage: Damage,
    at: Instant,
}

/// Where one seat's content comes from, handed in **unevaluated**.
///
/// The whole point of the enum is that it is cheap to build and expensive to
/// *use*: a caller assembles one of these per seat of every visible card every
/// frame, and the walk over a files tree or the climb up a terminal's grid
/// happens only on the far side of the gates. Passing the rows in already
/// gathered would move the cost above the budget, where no gate can refuse it.
pub enum SeatSource<'a> {
    Terminal(&'a DualPlaneSession),
    Files {
        state: &'a FilesLeafState,
        cache: &'a DirCache,
    },
    /// A preview pane, a commit-graph document, a placeholder — see
    /// [`MiniSeatContent::Face`].
    ///
    /// Owned, where the other two are borrowed, and the asymmetry is the point:
    /// a face's two words **are** its damage key, so there is nothing behind them
    /// to defer reading. Both are short and there are never many preview seats on
    /// screen at once, which is what makes it cheap enough to be the honest
    /// shape.
    Face {
        name: String,
        kind: String,
    },
}

/// One seat of one visible card, and how wide its own rectangle is in characters.
///
/// No `kind` beside the id: [`SeatSource`] already discriminates, and carrying
/// the tree's word for it as well would be a second answer to "what is this
/// seat" that a caller could get wrong in one place and right in the other.
pub struct SeatDemand<'a> {
    pub id: SeatId,
    /// How many characters fit across this seat's mini rectangle at the mini
    /// font's advance — §3.3's "same session, another width", as a cut.
    pub columns: usize,
    pub source: SeatSource<'a>,
}

/// Every window's own thumbnails, and the budget that fills them.
///
/// **One of these per window**, held on `WindowRuntime` beside the bit that says
/// whether the mode is on. Multi-window falls out of that and needs no rule of
/// its own: a window only ever walks its own tab list, so a second window's
/// forty tabs cost the first window nothing, and closing a window takes its
/// projections with it.
#[derive(Debug, Default)]
pub struct FocusThumbnails {
    /// The content each visible card is showing, by tab and then by seat.
    ///
    /// Handed out by reference to the chrome builder, which is why the content
    /// lives in its own map: `seats::FocusThumbnail` borrows exactly this, and a
    /// map that also carried the bookkeeping would make the painter's type
    /// depend on how the budget happens to be kept.
    content: BTreeMap<TabId, BTreeMap<SeatId, MiniSeatContent>>,
    /// The bookkeeping, flat, so that the map above stays the shape the painter
    /// wants.
    entries: BTreeMap<(TabId, SeatId), Entry>,
    stats: ThumbStats,
}

impl FocusThumbnails {
    /// Throw everything away — gate 1, and the whole of "leaving the mode gives
    /// the cost back".
    ///
    /// Reports whether anything was actually held, so a caller can tell the
    /// difference between leaving the mode and being outside it, and so the
    /// no-op path really is a no-op.
    pub fn clear(&mut self) -> bool {
        let held = !self.content.is_empty() || !self.entries.is_empty();
        self.content.clear();
        self.entries.clear();
        held
    }

    /// What the budget has done since this window opened.
    ///
    /// Cumulative and never reset, which is what makes the dump readable: a
    /// reader watching `BT_FOCUS_THUMB_DUMP` scroll past is asking whether a
    /// number is *still moving*, and a counter that periodically went back to
    /// zero would answer that with a shrug. The tests take differences.
    #[must_use]
    pub fn stats(&self) -> ThumbStats {
        self.stats
    }

    /// What one tab's card is showing, or `None` if it is not being projected.
    #[must_use]
    pub fn seats(&self, tab: TabId) -> Option<&BTreeMap<SeatId, MiniSeatContent>> {
        self.content.get(&tab)
    }

    /// **Gate 2** — forget every card that is no longer visible.
    ///
    /// Called once per frame with the tabs whose cards are inside the column's
    /// clip box. A card that scrolled out is dropped entirely rather than left to
    /// go stale: the memory is small, but a kept entry is a card that would come
    /// back showing a minute-old picture for however long the throttle takes to
    /// notice, and "scroll back and see what it says now" is the behaviour the
    /// ruling asks for.
    pub fn retain_visible(&mut self, visible: &BTreeSet<TabId>) {
        let before = self.content.len();
        self.content.retain(|tab, _| visible.contains(tab));
        if self.content.len() != before {
            self.stats.dropped_offscreen += (before - self.content.len()) as u64;
            self.entries.retain(|(tab, _), _| visible.contains(tab));
        }
    }

    /// **Gates 3 and 4** — bring one visible card's seats up to date.
    ///
    /// `demands` is the tab's seats in tree order, already carrying their own
    /// widths. Seats not in the list are dropped, which is how a pane closed in a
    /// background tab stops being drawn on its card.
    pub fn project(&mut self, tab: TabId, demands: &[SeatDemand<'_>], now: Instant) {
        let content = self.content.entry(tab).or_default();
        let live: BTreeSet<SeatId> = demands.iter().map(|demand| demand.id).collect();
        content.retain(|seat, _| live.contains(seat));
        self.entries
            .retain(|(entry_tab, seat), _| *entry_tab != tab || live.contains(seat));
        for demand in demands {
            let damage = demand.damage();
            let key = (tab, demand.id);
            match self.entries.get(&key) {
                // Gate 3: nothing behind this seat has moved. The one comparison
                // that makes an idle window free.
                Some(entry) if entry.damage == damage => {
                    self.stats.skipped_unchanged += 1;
                    continue;
                }
                // Gate 4: it has moved, but not long enough ago to be worth
                // redrawing. The old content stands, which is why this is a skip
                // and not a deferral — there is nothing queued and nothing to
                // flush.
                Some(entry) if now.duration_since(entry.at) < MIN_INTERVAL => {
                    self.stats.skipped_throttled += 1;
                    continue;
                }
                _ => {}
            }
            content.insert(demand.id, demand.project());
            self.entries.insert(key, Entry { damage, at: now });
            self.stats.projections += 1;
        }
    }
}

impl SeatDemand<'_> {
    /// This seat's damage key — the cheap half, asked on every frame of every
    /// visible card.
    ///
    /// Everything in it is a read of something already in memory: a counter, a
    /// small `Eq` value, two strings. Nothing here walks a tree or touches a
    /// grid, which is the property that makes gate 3 worth having at all.
    fn damage(&self) -> Damage {
        match &self.source {
            SeatSource::Terminal(session) => Damage::Grid {
                revision: session.screen_revision(),
                columns: self.columns,
            },
            SeatSource::Files { state, cache } => Damage::Files {
                revision: cache.revision(),
                state: (*state).clone(),
            },
            SeatSource::Face { name, kind } => Damage::Face {
                name: name.clone(),
                kind: kind.clone(),
            },
        }
    }

    /// The expensive half: build the seat's content. Reached only past all four
    /// gates.
    fn project(&self) -> MiniSeatContent {
        match &self.source {
            SeatSource::Terminal(session) => {
                MiniSeatContent::Transcript(transcript_tail(session, self.columns))
            }
            SeatSource::Files { state, cache } => MiniSeatContent::Files(files_head(state, cache)),
            SeatSource::Face { name, kind } => MiniSeatContent::Face {
                name: name.clone(),
                kind: kind.clone(),
            },
        }
    }
}

/// **How many characters fit across one mini seat** — §3.3's second width, as
/// the number a session's tail is cut to.
///
/// `advance` is one character of the mini transcript's face, measured by the
/// renderer (`WindowRuntime::focus_mini_advance`); the face is monospaced, so one
/// character's advance is every character's, and the division is exact rather
/// than a guessed ratio.
///
/// **One over the count that fits, deliberately.** The row is clipped at the
/// seat's edge, so the character straddling that edge is drawn as far as the box
/// goes and cut there — which is what `overflow: hidden` does to a line of text
/// and what makes a cut line look cut rather than look short. Dropping it would
/// leave up to a character's worth of blank inside a box the text overflows.
#[must_use]
pub fn mini_columns(rect: [f32; 4], advance: f32, scale: f32) -> usize {
    let border = (bt_render::FOCUS_MINI_BORDER_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let pad = (bt_render::FOCUS_MINI_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let width = rect[2] - rect[0] - 2.0 * (border + pad);
    // A face nobody has measured yet is `0.0`, and one measured before the glyph
    // cache was ready could be anything: both answer "no columns" rather than
    // dividing, because a column count arrived at by dividing by a number that is
    // not a width is a cut in the wrong place, which shows on screen.
    if width <= 0.0 || advance <= 0.0 || !advance.is_finite() {
        return 0;
    }
    (width / advance) as usize + 1
}

/// The last [`FOCUS_MINI_TERM_ROWS`] rows of a session's screen **that have
/// anything on them**, oldest first, each cut to `columns` characters.
///
/// **Trailing blanks are skipped, and that is the difference between a
/// thumbnail and a picture of the bottom of a rectangle.** A shell that has not
/// filled its screen yet leaves every row under its prompt empty; the six
/// *bottom* rows of that grid are six empty rows, which is a true statement about
/// the grid and tells a reader nothing at all. The mock-up's own model is a list
/// of lines with no blank padding in it (`s.lines.slice(-6)`), and this is that
/// list, found by walking up from the floor.
///
/// The walk is bounded by the grid and is only ever reached when the generation
/// moved, so a session with nothing happening in it does not walk at all — and a
/// session busy enough to walk far is, by the time it is, a session whose screen
/// has filled and whose walk is one row long.
fn transcript_tail(session: &DualPlaneSession, columns: usize) -> Vec<String> {
    let (_, rows) = session.live_dimensions();
    let mut tail: Vec<String> = Vec::with_capacity(FOCUS_MINI_TERM_ROWS);
    for row in (0..rows.get()).rev() {
        let Some(captured) = session.live_row(row) else {
            continue;
        };
        let text: String = captured
            .cells
            .iter()
            // A wide character's spacer has no text of its own; taking its empty
            // string would drop a column out of the middle of the line and pull
            // everything after it one place left, which on a table is the whole
            // table coming apart.
            .filter(|cell| !cell.wide_spacer)
            .flat_map(|cell| cell.text.chars())
            .collect();
        let text = text.trim_end();
        // Still climbing past the blank floor: nothing has been kept yet, so an
        // empty row is not a blank line inside the tail, it is the floor.
        if text.is_empty() && tail.is_empty() {
            continue;
        }
        tail.push(cut_to(text, columns));
        if tail.len() == FOCUS_MINI_TERM_ROWS {
            break;
        }
    }
    tail.reverse();
    tail
}

/// `text`, cut to `columns` characters.
///
/// **By character and not by byte**: the cut is the card's width in cells, and a
/// byte cut through a multi-byte character would not even be a string. Counted in
/// `char`s rather than graphemes because the advance being counted against is a
/// monospaced cell's, which is what the grid the text came out of already
/// measured it in.
fn cut_to(text: &str, columns: usize) -> String {
    match text.char_indices().nth(columns) {
        Some((end, _)) => text[..end].to_owned(),
        None => text.to_owned(),
    }
}

/// The first [`FOCUS_MINI_FILES_ROWS`] rows a files column is showing.
///
/// The column's **own** walk (`files::tree_view`), so a card can never show a
/// tree the pane under it disagrees with — and it asks for nothing: `tree_view`
/// returns the directories it would like read next and this drops them on the
/// floor, because a thumbnail must not put a question to the disk. A folder
/// nobody has opened stays a folder nobody has opened.
fn files_head(state: &FilesLeafState, cache: &DirCache) -> Vec<MiniFilesRow> {
    files::tree_view(state, cache)
        .rows
        .into_iter()
        .take(FOCUS_MINI_FILES_ROWS)
        .map(|row| MiniFilesRow {
            directory: matches!(row.kind, RowKind::Directory { .. }),
            depth: u16::try_from(row.depth).unwrap_or(u16::MAX),
            name: row.name,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    fn tab(id: u64) -> TabId {
        TabId(id)
    }

    fn seat(id: u64) -> SeatId {
        SeatId(id)
    }

    /// A seat whose whole content is two words — enough to drive every gate,
    /// because a face's damage key **is** its content and can be changed by
    /// changing one letter.
    fn face(id: u64, name: &str) -> SeatDemand<'static> {
        SeatDemand {
            id: seat(id),
            columns: 40,
            source: SeatSource::Face {
                name: name.to_owned(),
                kind: "TXT".to_owned(),
            },
        }
    }

    /// A shell 40 columns wide and 10 rows tall, with nothing on it yet.
    fn session() -> DualPlaneSession {
        DualPlaneSession::new(
            NonZeroU32::new(40).expect("40 is not zero"),
            NonZeroU32::new(10).expect("10 is not zero"),
        )
    }

    #[test]
    fn the_first_sight_of_a_card_projects_every_seat_it_has() {
        let mut thumbs = FocusThumbnails::default();
        let now = Instant::now();
        thumbs.project(tab(1), &[face(1, "a"), face(2, "b")], now);
        assert_eq!(thumbs.stats().projections, 2);
        assert_eq!(thumbs.seats(tab(1)).map(BTreeMap::len), Some(2));
    }

    /// **Gate 3, and the claim the whole budget is for**: a window nobody is
    /// touching does no work at all, however many frames go by.
    ///
    /// A hundred frames, not two, and each one a *later* instant — so the
    /// throttle cannot be what is holding the work back. What holds it back is
    /// that nothing changed, which is the only reason that goes on working after
    /// the tenth of a second is up.
    #[test]
    fn an_idle_card_projects_once_and_then_never_again() {
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[face(1, "a"), face(2, "b")], start);
        assert_eq!(thumbs.stats().projections, 2);
        for frame in 1..=100 {
            thumbs.project(
                tab(1),
                &[face(1, "a"), face(2, "b")],
                start + MIN_INTERVAL * frame,
            );
        }
        let stats = thumbs.stats();
        assert_eq!(
            stats.projections, 2,
            "a hundred frames of nothing happening cost nothing"
        );
        assert_eq!(stats.skipped_unchanged, 200);
        assert_eq!(stats.skipped_throttled, 0, "the clock never had to refuse");
    }

    /// **Gate 4** — a seat changing on every frame still only rebuilds at
    /// [`MIN_INTERVAL`].
    #[test]
    fn a_seat_changing_on_every_frame_rebuilds_at_the_ceiling_and_no_faster() {
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        // One frame every 10ms for a second: a hundred frames, ten intervals.
        let frame = Duration::from_millis(10);
        for tick in 0..100 {
            thumbs.project(
                tab(1),
                &[face(1, &format!("line {tick}"))],
                start + frame * tick,
            );
        }
        let stats = thumbs.stats();
        assert_eq!(
            stats.projections, 10,
            "one second of continuous change is ten projections at 10Hz"
        );
        assert_eq!(stats.skipped_throttled, 90);
        assert_eq!(
            stats.skipped_unchanged, 0,
            "every frame really did carry something new"
        );
    }

    /// The throttle refuses; it does not queue. What the card shows in between is
    /// the last thing that was built — not a blank, and not a stale-marked box.
    #[test]
    fn a_throttled_seat_goes_on_showing_what_it_last_said() {
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[face(1, "first")], start);
        thumbs.project(
            tab(1),
            &[face(1, "second")],
            start + Duration::from_millis(1),
        );
        assert_eq!(
            thumbs.seats(tab(1)).and_then(|seats| seats.get(&seat(1))),
            Some(&MiniSeatContent::Face {
                name: "first".to_owned(),
                kind: "TXT".to_owned(),
            })
        );
    }

    /// **Gate 2** — a card scrolled out of the column stops being projected, and
    /// scrolling back projects it *fresh* rather than showing what it said when
    /// it left.
    #[test]
    fn a_card_scrolled_out_of_the_column_is_dropped_and_comes_back_new() {
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[face(1, "a")], start);
        thumbs.project(tab(2), &[face(1, "b")], start);
        thumbs.retain_visible(&BTreeSet::from([tab(1), tab(2)]));
        assert_eq!(thumbs.stats().projections, 2);

        // Tab 2's card scrolls out of the clip box.
        thumbs.retain_visible(&BTreeSet::from([tab(1)]));
        assert!(
            thumbs.seats(tab(2)).is_none(),
            "an off-screen card holds nothing"
        );
        assert_eq!(thumbs.stats().dropped_offscreen, 1);

        // A minute passes with it off screen; nothing about it costs anything,
        // because the caller does not even ask.
        let later = start + Duration::from_secs(60);
        thumbs.project(tab(1), &[face(1, "a")], later);
        assert_eq!(
            thumbs.stats().projections,
            2,
            "the card still on screen had nothing new to say either"
        );

        // Scrolled back: it projects again, and it is not the throttle that let
        // it — the entry it would have been throttled against went with it.
        thumbs.project(tab(2), &[face(1, "b")], later);
        thumbs.retain_visible(&BTreeSet::from([tab(1), tab(2)]));
        assert_eq!(thumbs.stats().projections, 3);
        assert!(thumbs.seats(tab(2)).is_some());
    }

    /// **Gate 1** — leaving the mode gives the whole cost back, and the next
    /// frame is a frame with no budget in it.
    #[test]
    fn leaving_the_mode_empties_everything() {
        let mut thumbs = FocusThumbnails::default();
        thumbs.project(tab(1), &[face(1, "a")], Instant::now());
        assert!(thumbs.clear(), "there was something to give back");
        assert!(thumbs.seats(tab(1)).is_none());
        assert!(
            !thumbs.clear(),
            "a window outside the mode has nothing to clear on every frame"
        );
    }

    /// A pane closed in a background tab stops being drawn on that tab's card:
    /// the seat is gone from the demand, so it goes from the content too.
    #[test]
    fn a_seat_that_left_the_tree_leaves_the_card() {
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[face(1, "a"), face(2, "b")], start);
        thumbs.project(tab(1), &[face(1, "a")], start + Duration::from_secs(1));
        let seats = thumbs.seats(tab(1)).expect("the card is still projected");
        assert_eq!(seats.len(), 1);
        assert!(seats.contains_key(&seat(1)));
    }

    /// **Gate 3 on a real grid**: the generation is what says whether a shell has
    /// anything new, and re-reading a quiet one is refused before a single cell
    /// is touched.
    #[test]
    fn a_quiet_shell_is_refused_on_its_generation() {
        let mut shell = session();
        shell
            .feed(b"hello\r\n")
            .expect("a shell takes its own output");
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        fn demand(shell: &DualPlaneSession) -> SeatDemand<'_> {
            SeatDemand {
                id: seat(1),
                columns: 40,
                source: SeatSource::Terminal(shell),
            }
        }
        thumbs.project(tab(1), &[demand(&shell)], start);
        assert_eq!(thumbs.stats().projections, 1);
        for frame in 1..=20 {
            thumbs.project(tab(1), &[demand(&shell)], start + MIN_INTERVAL * frame);
        }
        assert_eq!(
            thumbs.stats().projections,
            1,
            "twenty frames over a shell that said nothing"
        );
        assert_eq!(thumbs.stats().skipped_unchanged, 20);

        // It says something, and the next frame past the ceiling picks it up.
        shell
            .feed(b"world\r\n")
            .expect("a shell takes its own output");
        thumbs.project(tab(1), &[demand(&shell)], start + MIN_INTERVAL * 21);
        assert_eq!(thumbs.stats().projections, 2);
    }

    /// A card that got narrower re-cuts even though the grid behind it did not
    /// move — which is the half of §3.3 this slice actually uses.
    #[test]
    fn a_narrower_card_re_cuts_a_grid_that_did_not_change() {
        let shell = session();
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        let demand = |columns| SeatDemand {
            id: seat(1),
            columns,
            source: SeatSource::Terminal(&shell),
        };
        thumbs.project(tab(1), &[demand(40)], start);
        thumbs.project(tab(1), &[demand(40)], start + MIN_INTERVAL);
        assert_eq!(thumbs.stats().projections, 1);
        thumbs.project(tab(1), &[demand(20)], start + MIN_INTERVAL * 2);
        assert_eq!(thumbs.stats().projections, 2);
    }

    /// The tail is the tail of the **transcript** and not of the rectangle: a
    /// shell that has printed three lines onto a ten-row screen has three lines
    /// on its card, not seven blank ones and three.
    #[test]
    fn the_tail_skips_the_blank_floor_under_a_short_session() {
        let mut shell = session();
        shell
            .feed(b"one\r\ntwo\r\nthree\r\n")
            .expect("a shell takes its own output");
        assert_eq!(
            transcript_tail(&shell, 40),
            vec!["one".to_owned(), "two".to_owned(), "three".to_owned()]
        );
    }

    /// And it is the **last** six of them, oldest first, once there are more than
    /// six.
    #[test]
    fn the_tail_is_the_last_six_lines_in_reading_order() {
        let mut shell = session();
        for line in 1..=9 {
            shell
                .feed(format!("line {line}\r\n").as_bytes())
                .expect("a shell takes its own output");
        }
        assert_eq!(
            transcript_tail(&shell, 40),
            (4..=9)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
        );
    }

    /// The cut is the card's width, and it is a cut and not an ellipsis: the
    /// painter's clip is what makes it look cut, so nothing is added here.
    #[test]
    fn a_long_line_is_cut_to_the_cards_own_column_count() {
        let mut shell = session();
        shell
            .feed(b"abcdefghijklmnopqrstuvwxyz\r\n")
            .expect("a shell takes its own output");
        assert_eq!(transcript_tail(&shell, 8), vec!["abcdefgh".to_owned()]);
    }

    /// A cut that landed inside a character would not be a string at all.
    #[test]
    fn the_cut_counts_characters_and_never_bytes() {
        assert_eq!(cut_to("日本語のテキスト", 3), "日本語");
        assert_eq!(cut_to("ascii", 99), "ascii");
    }

    /// **The budget, in milliseconds** (§7.1.6b′ F2) — the two numbers the ruling
    /// asks somebody to set, written where they can be read and checked.
    ///
    /// They bound the cost **this module adds to a frame**, which is the whole of
    /// what F2 adds to one while the mode is on: nothing below here reaches the
    /// GPU, and an idle frame hands the painter strings it already had, so the
    /// label list is byte-identical and `set_chrome` uploads nothing.
    ///
    /// Deliberately loose against the measurements they were set from — see
    /// [`the_budget_holds_under_ten_tabs_of_full_blast_output`] for what was
    /// actually observed. A wall-clock assertion inside a test suite shares its
    /// machine with whatever else is running, so a ceiling set at the measurement
    /// would be a test that fails when the machine is busy rather than when the
    /// code is wrong.
    const FULL_BLAST_BUDGET_MS: f64 = 3.0;
    /// The same for a window nobody is touching. Gate 3 means the work here is
    /// twenty integer comparisons, so this is the *shape* of the claim rather
    /// than a real ceiling: it is three orders of magnitude above what an idle
    /// frame costs, and it would only ever be crossed by somebody putting real
    /// work back on the idle path.
    const IDLE_BUDGET_MS: f64 = 0.5;

    /// The shape §7.1.6b′ names: ten tabs, two seats each.
    const BUDGET_TABS: usize = 10;
    const BUDGET_SEATS_PER_TAB: usize = 2;

    /// The median of `samples`, which is what a frame budget is stated in — a
    /// mean would be moved by the one frame the scheduler took the core away.
    fn median(mut samples: Vec<f64>) -> f64 {
        samples.sort_by(f64::total_cmp);
        samples[samples.len() / 2]
    }

    /// Twenty shells, each with a screenful of output already on it.
    fn budget_fleet() -> Vec<Vec<DualPlaneSession>> {
        (0..BUDGET_TABS)
            .map(|_| {
                (0..BUDGET_SEATS_PER_TAB)
                    .map(|_| {
                        let mut shell = DualPlaneSession::new(
                            NonZeroU32::new(120).expect("120 is not zero"),
                            NonZeroU32::new(40).expect("40 is not zero"),
                        );
                        for line in 0..60 {
                            shell
                                .feed(format!("   Compiling some-crate v0.{line}.0\r\n").as_bytes())
                                .expect("a shell takes its own output");
                        }
                        shell
                    })
                    .collect()
            })
            .collect()
    }

    /// **The full-blast ceiling.** Ten tabs of two seats, every one of them
    /// scrolling, every card visible: the per-frame cost of bringing the whole
    /// column up to date stays inside [`FULL_BLAST_BUDGET_MS`].
    ///
    /// Every frame writes to every shell, so gate 3 refuses nothing and gate 4 is
    /// what is actually being measured — which is the point: this is the worst
    /// case the mode has, and it is bounded by the throttle rather than by luck.
    #[test]
    fn the_budget_holds_under_ten_tabs_of_full_blast_output() {
        let mut fleet = budget_fleet();
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        let frame = Duration::from_millis(8);
        let mut samples = Vec::with_capacity(200);
        for tick in 0..200 {
            // Outside the clock: this is the *child* writing, which happens
            // whether or not anybody is drawing a card of it.
            for seats in &mut fleet {
                for shell in seats {
                    shell
                        .feed(format!("   Compiling crate-{tick} v1.0.0\r\n").as_bytes())
                        .expect("a shell takes its own output");
                }
            }
            let now = start + frame * tick;
            let began = Instant::now();
            for (index, seats) in fleet.iter().enumerate() {
                let demands: Vec<SeatDemand<'_>> = seats
                    .iter()
                    .enumerate()
                    .map(|(seat_index, shell)| SeatDemand {
                        id: SeatId(seat_index as u64),
                        columns: 44,
                        source: SeatSource::Terminal(shell),
                    })
                    .collect();
                thumbs.project(TabId(index as u64), &demands, now);
            }
            samples.push(began.elapsed().as_secs_f64() * 1_000.0);
        }
        let observed = median(samples);
        // Printed as well as asserted: the ceiling above is loose on purpose, so
        // the number that actually says whether this slice got cheaper or dearer
        // is the measurement, and `--nocapture` is where a reader finds it.
        println!("F2 full blast: {observed:.4}ms median per frame");
        assert!(
            observed <= FULL_BLAST_BUDGET_MS,
            "ten tabs of two scrolling seats cost {observed:.4}ms per frame, \
             over the {FULL_BLAST_BUDGET_MS}ms this slice budgeted"
        );
    }

    /// **The idle floor, which is the claim that matters.** The same twenty seats
    /// with nothing happening in them: the mode adds essentially nothing to a
    /// frame, because the damage gate refuses every one of them.
    ///
    /// The counter assertion beside the clock is the real one — see
    /// [`an_idle_card_projects_once_and_then_never_again`] — and this one says
    /// the same thing in the unit the ruling asked for.
    #[test]
    fn an_idle_window_in_the_mode_costs_almost_nothing_per_frame() {
        let fleet = budget_fleet();
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        let frame = Duration::from_millis(8);
        let mut samples = Vec::with_capacity(200);
        for tick in 0..200 {
            let now = start + frame * tick;
            let began = Instant::now();
            for (index, seats) in fleet.iter().enumerate() {
                let demands: Vec<SeatDemand<'_>> = seats
                    .iter()
                    .enumerate()
                    .map(|(seat_index, shell)| SeatDemand {
                        id: SeatId(seat_index as u64),
                        columns: 44,
                        source: SeatSource::Terminal(shell),
                    })
                    .collect();
                thumbs.project(TabId(index as u64), &demands, now);
            }
            samples.push(began.elapsed().as_secs_f64() * 1_000.0);
        }
        let observed = median(samples);
        println!("F2 idle: {observed:.4}ms median per frame");
        assert!(
            observed <= IDLE_BUDGET_MS,
            "an idle window cost {observed:.4}ms per frame, over the \
             {IDLE_BUDGET_MS}ms budgeted"
        );
        let projected = thumbs.stats().projections;
        assert_eq!(
            projected,
            (BUDGET_TABS * BUDGET_SEATS_PER_TAB) as u64,
            "and it paid for exactly one projection per seat, on the first frame"
        );
    }

    /// The column count comes off the seat's own rectangle, and it is one over
    /// what fits so that the character straddling the edge is drawn and clipped
    /// rather than dropped.
    #[test]
    fn a_seats_column_count_is_read_off_its_own_rectangle() {
        // 100px wide, 1px border and 4px padding either side: 90px of run, at a
        // 5px advance, is eighteen whole characters and a nineteenth to cut.
        assert_eq!(mini_columns([0.0, 0.0, 100.0, 40.0], 5.0, 1.0), 19);
        // A seat with no room left for text asks for nothing.
        assert_eq!(mini_columns([0.0, 0.0, 8.0, 40.0], 5.0, 1.0), 0);
        // And a face nobody has measured yet cannot be divided by.
        assert_eq!(mini_columns([0.0, 0.0, 100.0, 40.0], 0.0, 1.0), 0);
    }
}
