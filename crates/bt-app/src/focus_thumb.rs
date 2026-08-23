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
//!    [`DirCache::revision`] together with the state that decides which page it
//!    is on and which of that page's rows are open, a preview buffer's identity
//!    and revision, a face's own two words.
//!    **This is the gate that makes an idle window free** — twenty seats with
//!    nothing happening in them answer twenty integer comparisons, produce no
//!    allocation, and hand back the same strings, so the label list the renderer
//!    is given is byte-identical and `set_chrome` uploads nothing.
//! 4. **The clock.** A seat that *is* changing re-projects at most once every
//!    [`MIN_INTERVAL`]. A shell writing at 10 000 lines a second and a shell
//!    writing at ten cost the same, because a card 160 pixels tall cannot show
//!    the difference and the eye cannot see it.
//!
//! The gates are in that order on purpose. Damage is asked *before* the clock
//! because reading a generation is free and refusing on it costs nothing; the
//! clock is asked after, because its job is to bound the *rate* of real work, not
//! to add a second reason to do none.
//!
//! # What one projection is
//!
//! One seat's content, rebuilt: **as many rows of a terminal's tail as that
//! seat's own rectangle holds**, cut to that seat's own column count; as many
//! rows of a files column's tree — or, when that column is standing on its Git
//! page instead, [`git_face`]'s two words; as many head lines of a preview's
//! loaded document; or a face's two words.
//! [`ThumbStats::projections`] counts exactly those, which is what the F2 tests
//! assert on — a counter, not a stopwatch, so the gates are checked for what they
//! are rather than for how fast the machine that ran them was.
//!
//! # What a projection could not answer
//!
//! The red line stands: **a projection never goes and fetches**. What it does,
//! since 2026-08-21, is hand back the list of things it had nothing in memory
//! for ([`UnreadDir`]), and the caller — which owns the workers and knows
//! whether the mode is even on — decides whether to go. That keeps the fetching
//! and the drawing on opposite sides of the same line they were always on, and
//! it costs nothing extra: the list falls out of the walk that drew the rows.
//!
//! # Where the width and the height come in
//!
//! §3.3's multi-projection — the same session presented at more than one width —
//! is the machine floor this stands on, and the shape it takes here is the
//! **cut**: each seat is handed the number of columns that fit across *its*
//! rectangle at the mini font's advance ([`mini_columns`]), and a row wider than
//! that is cut before it is ever shaped. That is why `columns` is part of a
//! terminal seat's damage key: a card that changed width has to re-cut even
//! though the grid behind it did not move.
//!
//! The height is the same argument stood on its end, and it arrived on
//! 2026-08-20 with the ruling that the column *is* a running terminal set
//! smaller: **how many rows a seat projects is how many rows it holds**
//! ([`mini_rows`]), which is a fact about that seat's rectangle and the line
//! height beside it, and never a constant. So `rows` sits in the damage key
//! beside `columns` for the same reason — a card that grew taller has to
//! re-project even though nothing behind it moved.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use bt_layout::{SeatId, SeatKind};
use bt_term::DualPlaneSession;

use crate::{
    TabId,
    files::{self, DirCache, RowKind},
    preview::{PreviewBuffer, PreviewSource},
    seats::{FilesLeafState, FilesView, MiniFilesRow, MiniSeatContent, seat_title},
};

/// **The rate ceiling: one projection per seat per 100 ms — 10 Hz.**
///
/// Chosen against the thing being drawn rather than against the display: a mini
/// seat carries a dozen-odd rows of 7.5px text inside a 160px card, and the
/// difference between a tail redrawn ten times a second and one redrawn a
/// hundred times is not visible at that size — it is only payable. Ten is also comfortably above
/// the rate at which a reader can follow a scrolling card at all, so nothing
/// legible is being withheld.
///
/// It bounds the **worst** case and not the common one: gate 3 above means a
/// quiet seat does not project at 10 Hz, it does not project at all.
pub const MIN_INTERVAL: Duration = Duration::from_millis(100);

use bt_render::{
    FOCUS_MINI_FILES_FONT_LOGICAL_PX, FOCUS_MINI_FILES_LINE_HEIGHT,
    FOCUS_MINI_TERM_FONT_LOGICAL_PX, FOCUS_MINI_TERM_LINE_HEIGHT,
};

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
    /// A terminal seat: the screen's own damage counter, and the **shape** it was
    /// projected into.
    ///
    /// All three, because any one of them can change the strings on its own —
    /// new output moves the tail, a narrower card cuts it shorter, and a shorter
    /// card carries fewer rows of it.
    ///
    /// `screen_revision` and **not** `grid_generation`, which is the near-miss
    /// this branch actually made first: that one counts reflow boundaries, so it
    /// stands perfectly still through a shell printing all day and a card keyed
    /// to it never updates at all. See `DualPlaneSession::screen_revision`.
    Grid {
        revision: u64,
        columns: usize,
        rows: usize,
        /// **Where the window is aimed**, in rows above the tail — see
        /// [`SeatSource::Terminal`]. A fourth thing that changes the strings on
        /// its own: nothing behind the seat has to move for a reader who has
        /// just turned the wheel to be looking at different rows.
        skip: usize,
    },
    /// A files column: the cache's write counter, and the state that decides
    /// **which page it is on** and which of that page's rows are visible.
    ///
    /// The state is carried whole rather than hashed. It is a root, a page, an
    /// open set and a selection — small, `Eq`, and already cloned by every other
    /// reader of it — while a hash would be a second answer to "did this change"
    /// that can be wrong in the direction that matters.
    ///
    /// **Carrying it whole is also what makes turning the column over repaint
    /// its card.** `view` is one of its fields, so the page a column is standing
    /// on is part of what its card was built from without this gate having to
    /// learn a second fact — which is exactly the property a narrower key would
    /// have thrown away, and the bug this shape prevented from having a second
    /// life when the projection learned about the Git page.
    ///
    /// It is deliberately a **superset** of what the content depends on: a
    /// column on its Git page draws nothing out of the directory cache, so a
    /// `read_dir` landing under it re-projects a face that cannot have changed.
    /// That costs one rebuild of two short strings and hands back the same
    /// bytes, so the label list is still byte-identical and `set_chrome` still
    /// uploads nothing. A key that was a *subset* would be a card that lies,
    /// which is the asymmetry this errs on the safe side of.
    Files {
        revision: u64,
        state: FilesLeafState,
        rows: usize,
    },
    /// A preview seat drawing its document: **which** document, how many times
    /// its body has changed, and the shape it was cut into.
    ///
    /// The identity is carried beside the revision and not instead of it,
    /// because a revision counter is per-buffer and starts at zero: a pane
    /// switched from one freshly-read file to another would otherwise present
    /// the same key and go on showing the first file's head. That is the same
    /// asymmetry [`Self::Files`] errs on — a superset costs one rebuild, a
    /// subset is a card that lies.
    ///
    /// `mono` is here because it is the *view's* and not the buffer's: flipping
    /// a markdown pane to its source changes both the face the head lines are
    /// set in and the width they are cut to, with nothing behind the buffer
    /// moving at all.
    Document {
        source: PreviewSource,
        revision: u64,
        columns: usize,
        rows: usize,
        mono: bool,
    },
    /// A face: the two words on it. There is nothing behind them to have a
    /// generation, so they *are* the generation.
    Face { name: String, kind: String },
    /// **A page's last frame** — how many frames this seat has had, and the box
    /// it is drawn in (W2 slice ⑥, §7.11).
    ///
    /// The serial and *not* the pixels: `web_thumb` bumps it once per picture
    /// stored, so a card re-projects when the frame is replaced and on no other
    /// occasion. Comparing the bytes would be a megabyte-wide memcmp on every
    /// frame of every visible card to answer a question a counter already
    /// answers, and comparing the `Arc` would make "the same picture, moved" and
    /// "a new picture" the same event.
    ///
    /// **No rectangle**, unlike every other variant here, and the exception is
    /// exact rather than an oversight: a projected *row* has to be re-cut when
    /// its box changes, and a texture does not — the painter is handed the box
    /// the card gives it this frame and the picture is sampled into it. A card
    /// that grew therefore draws a bigger picture without rebuilding anything,
    /// which is the whole difference between pixels and rows.
    Page { key: String },
}

/// **A directory one card's files seat had nothing in memory to draw** (user
/// ruling 2026-08-21).
///
/// The projection asks nobody anything — that is §7.1.6b′'s red line and it has
/// not moved. What it does now is *say what it lacked*: [`files::tree_view`]
/// already hands back the keys it could not answer (`TreeView::wanted`, the very
/// list the docked walk feeds `Runtime::files_trees` from), and throwing that
/// away here was what left a background tab's card sitting on `Loading…` for as
/// long as nobody clicked the tab.
///
/// The caller owns the worker and decides whether to go. It is produced only by
/// a projection that actually ran, so the four gates bound how often it can be
/// produced exactly as they bound the walk that produces it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnreadDir {
    pub seat: SeatId,
    /// The tree key, in [`files::tree_view`]'s own grammar — `""` is the root.
    pub key: String,
}

/// One seat's entry: what it says, what that was built from, and when.
#[derive(Clone, Debug)]
struct Entry {
    damage: Damage,
    at: Instant,
    /// **A gesture is owed this seat a picture** — see
    /// [`FocusThumbnails::unthrottle`]. Set by a hand, cleared by the very next
    /// pass that looks at this seat, so it buys exactly one projection and never
    /// takes the seat off the clock.
    unthrottled: bool,
}

/// Where one seat's content comes from, handed in **unevaluated**.
///
/// The whole point of the enum is that it is cheap to build and expensive to
/// *use*: a caller assembles one of these per seat of every visible card every
/// frame, and the walk over a files tree or the climb up a terminal's grid
/// happens only on the far side of the gates. Passing the rows in already
/// gathered would move the cost above the budget, where no gate can refuse it.
pub enum SeatSource<'a> {
    /// A terminal seat, and **where down its screen the card's window is
    /// aimed** (user ruling 2026-08-21, §7.1.6b′ 「卡片窗口瞄准」).
    ///
    /// `skip` is a count of rows *above the tail*: `0` is the tail itself, which
    /// is what every card showed before the ruling and what every seat still
    /// opens at. Anything above zero lifts the window that many rows off the
    /// bottom of the screen, which is the whole of the feature: a program with
    /// fixed furniture along its floor — an agent's status block, `vim`'s status
    /// line, `lazygit`'s footer — puts its newest real output a *constant*
    /// distance above that floor, so one number aimed once keeps pointing at it.
    ///
    /// **It recognises no program**, and that is the ruling rather than an
    /// omission: a build that detected status bars would be a build that has to
    /// be taught each new one, would be wrong about the ones it had not met, and
    /// would move a reader's card without being asked. A number the reader sets
    /// with the wheel is right for whatever is actually running.
    Terminal {
        session: &'a DualPlaneSession,
        skip: usize,
    },
    Files {
        state: &'a FilesLeafState,
        cache: &'a DirCache,
    },
    /// **A preview pane with a body already in memory** (user ruling,
    /// 2026-08-20, reopening F2's own v1 decision).
    ///
    /// v1 drew every preview seat as a face, and the sentence it gave was "a
    /// page of prose at 7.5px is a grey smear". The ruling that reopened it is
    /// that the same sentence would condemn the terminal tail beside it, which
    /// nobody has ever wanted removed: prose at 7.5px and a compiler's output at
    /// 7.5px are equally legible, and the card's job is to say *which tab this
    /// is*, which the first lines of the document do far better than its
    /// extension in capitals.
    ///
    /// **It still does not ask the disk.** `DESIGN.md` §7.1.6b′'s red line —
    /// "a thumbnail must not put a question to the disk" — is what decides which
    /// preview seats reach this variant at all: a buffer already loaded into the
    /// tab's pool has its head in memory and can be quoted for free, and a pane
    /// whose buffer has not arrived (a background tab that has never been
    /// looked at) stays a [`Self::Face`]. Nothing here starts a read.
    ///
    /// The two things that are not text stay faces as well, and for the reason
    /// they always were: a commit graph is a picture, and a placeholder has no
    /// document behind it to quote.
    Document {
        buffer: &'a PreviewBuffer,
        /// Whether this document is set in the terminal's face or the window's
        /// — the ruling's "code monospaced, prose in the app font", decided
        /// where the *view* is known ([`crate::preview::PreviewView`]) and
        /// carried here as one bit.
        mono: bool,
    },
    /// A commit-graph document, a picture, a placeholder, a preview whose body
    /// has not arrived — see [`MiniSeatContent::Face`].
    ///
    /// Owned, where the other two are borrowed, and the asymmetry is the point:
    /// a face's two words **are** its damage key, so there is nothing behind them
    /// to defer reading. Both are short and there are never many preview seats on
    /// screen at once, which is what makes it cheap enough to be the honest
    /// shape.
    Face { name: String, kind: String },
    /// **A web seat with a frame in hand** (W2 slice ⑥, §7.11).
    ///
    /// The one variant whose content this module did not compute and cannot: a
    /// page's pixels come from `ICoreWebView2::CapturePreview`, arrive tens of
    /// milliseconds after they are asked for, and exist **only while the page is
    /// on the glass** — a hidden WebView never answers at all. So `web_thumb`
    /// owns the asking and the keeping, and what reaches the projection is the
    /// frame it already has.
    ///
    /// A web seat with **no** frame does not reach this variant. It is a
    /// [`Self::Face`], exactly as it was before this slice, and that is the
    /// honest picture rather than a fallback: there is nothing to draw, because
    /// nothing has ever been on that glass while anybody was looking.
    Page {
        picture: &'a crate::web_thumb::Picture,
    },
}

/// One seat of one visible card, and how wide its own rectangle is in characters.
///
/// No `kind` beside the id: [`SeatSource`] already discriminates, and carrying
/// the tree's word for it as well would be a second answer to "what is this
/// seat" that a caller could get wrong in one place and right in the other.
pub struct SeatDemand<'a> {
    pub id: SeatId,
    /// How many columns fit across this seat's mini rectangle at its own face's
    /// advance — §3.3's "same session, another width", as a cut. See
    /// [`mini_columns`].
    pub columns: usize,
    /// How many whole lines fit down this seat's mini rectangle at its own
    /// face's line height. See [`mini_rows`].
    pub rows: usize,
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

    /// **The gesture channel** — one seat, one picture, now (user report
    /// 2026-08-21).
    ///
    /// Gate 4's argument, written at the top of this file, is entirely about a
    /// *shell*: "a shell writing at 10 000 lines a second and a shell writing at
    /// ten cost the same, because a card 160 pixels tall cannot show the
    /// difference". Every word of that is true and none of it is about a
    /// **hand**. When the reader turns the wheel over a mini seat and moves that
    /// seat's window (`§7.1.6b′` ③ `card_skip`), the picture *is* the answer to
    /// the gesture, and a tenth of a second of not answering is the surface
    /// feeling stuck — which is exactly what the report said it felt like.
    ///
    /// So the clock is spent rather than lowered, and the exception is drawn as
    /// narrowly as the reason allows:
    ///
    /// * **The clock only.** Gate 3 is untouched: an aim that left the seat
    ///   saying the same thing — a window driven past the top of a short screen,
    ///   say — still rebuilds nothing, because there is nothing new to draw.
    /// * **One seat.** The credit is granted to the seat the pointer was over,
    ///   never to a pass, so a wheel spun over one card cannot pull twenty other
    ///   seats through the clock alongside it.
    /// * **Once.** The next pass that looks at this seat clears it, so a gesture
    ///   buys one projection and the shell behind that seat goes back to 10 Hz
    ///   immediately.
    ///
    /// The rate this opens is a hand's: one row per detent
    /// ([`crate::CardAim`] sees to it that a run of sub-detent reports is one
    /// row and not six), which is tens per second at the arm's limit against the
    /// hundreds per second [`MIN_INTERVAL`] is sized to refuse — and one seat's
    /// worth of them, not twenty.
    ///
    /// A seat with no entry is a seat the clock is not refusing anyway, so this
    /// is a no-op there rather than a booking made in advance.
    pub fn unthrottle(&mut self, tab: TabId, seat: SeatId) {
        if let Some(entry) = self.entries.get_mut(&(tab, seat)) {
            entry.unthrottled = true;
        }
    }

    /// **Gates 3 and 4** — bring one visible card's seats up to date.
    ///
    /// `demands` is the tab's seats in tree order, already carrying their own
    /// widths. Seats not in the list are dropped, which is how a pane closed in a
    /// background tab stops being drawn on its card.
    ///
    /// **What comes back is what the projection could not answer out of memory**
    /// — see [`UnreadDir`]. It is empty on every frame that projected nothing,
    /// which is most of them, and an empty `Vec` allocates nothing.
    pub fn project(
        &mut self,
        tab: TabId,
        demands: &[SeatDemand<'_>],
        now: Instant,
    ) -> Vec<UnreadDir> {
        let mut unread = Vec::new();
        let content = self.content.entry(tab).or_default();
        let live: BTreeSet<SeatId> = demands.iter().map(|demand| demand.id).collect();
        content.retain(|seat, _| live.contains(seat));
        self.entries
            .retain(|(entry_tab, seat), _| *entry_tab != tab || live.contains(seat));
        for demand in demands {
            let damage = demand.damage();
            let key = (tab, demand.id);
            // The credit a gesture left on this seat, taken as it is read: it
            // buys the one pass it is looked at by, whichever way that pass then
            // goes. See [`Self::unthrottle`].
            let mut unthrottled = false;
            if let Some(entry) = self.entries.get_mut(&key) {
                unthrottled = std::mem::take(&mut entry.unthrottled);
            }
            match self.entries.get(&key) {
                // Gate 3: nothing behind this seat has moved. The one comparison
                // that makes an idle window free. **Asked of a gesture too** —
                // an aim that changed nothing has nothing to draw.
                Some(entry) if entry.damage == damage => {
                    self.stats.skipped_unchanged += 1;
                    continue;
                }
                // Gate 4: it has moved, but not long enough ago to be worth
                // redrawing. The old content stands, which is why this is a skip
                // and not a deferral — there is nothing queued and nothing to
                // flush. A hand that just moved this seat's own window is the
                // one thing this gate has no argument against.
                Some(entry) if !unthrottled && now.duration_since(entry.at) < MIN_INTERVAL => {
                    self.stats.skipped_throttled += 1;
                    continue;
                }
                _ => {}
            }
            let (projected, wanted) = demand.project();
            unread.extend(wanted.into_iter().map(|key| UnreadDir {
                seat: demand.id,
                key,
            }));
            content.insert(demand.id, projected);
            self.entries.insert(
                key,
                Entry {
                    damage,
                    at: now,
                    unthrottled: false,
                },
            );
            self.stats.projections += 1;
        }
        unread
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
            SeatSource::Terminal { session, skip } => Damage::Grid {
                revision: session.screen_revision(),
                columns: self.columns,
                rows: self.rows,
                skip: *skip,
            },
            SeatSource::Files { state, cache } => Damage::Files {
                revision: cache.revision(),
                state: (*state).clone(),
                rows: self.rows,
            },
            SeatSource::Document { buffer, mono } => Damage::Document {
                source: buffer.source.clone(),
                revision: buffer.revision,
                columns: self.columns,
                rows: self.rows,
                mono: *mono,
            },
            SeatSource::Face { name, kind } => Damage::Face {
                name: name.clone(),
                kind: kind.clone(),
            },
            SeatSource::Page { picture } => Damage::Page {
                key: picture.key.clone(),
            },
        }
    }

    /// The expensive half: build the seat's content. Reached only past all four
    /// gates.
    ///
    /// **And the directories the build had nothing for** — see [`UnreadDir`].
    /// They come back from the same walk that drew the rows rather than from a
    /// second one, so asking what a card is missing costs nothing beyond drawing
    /// it and cannot disagree with what it drew.
    fn project(&self) -> (MiniSeatContent, Vec<String>) {
        match &self.source {
            SeatSource::Terminal { session, skip } => {
                let (lines, more_below) = transcript_tail(session, self.columns, self.rows, *skip);
                (
                    MiniSeatContent::Transcript { lines, more_below },
                    Vec::new(),
                )
            }
            SeatSource::Document { buffer, mono } => (
                MiniSeatContent::Document {
                    lines: document_head(buffer, self.columns, self.rows),
                    mono: *mono,
                },
                Vec::new(),
            ),
            // **The page the column is on, and not the one it has** (§7.1.3g's
            // ruling, one surface further out): a Files seat is a column with
            // two pages and `view` says which of them is on screen right now.
            SeatSource::Files { state, cache } => match state.view {
                FilesView::Files => {
                    let (rows, wanted) = files_head(state, cache, self.rows);
                    (MiniSeatContent::Files(rows), wanted)
                }
                // A column standing on its Git page draws two words out of its
                // own state and reads no directory, so there is nothing here it
                // could be missing.
                FilesView::Git => (git_face(state), Vec::new()),
            },
            SeatSource::Face { name, kind } => (
                MiniSeatContent::Face {
                    name: name.clone(),
                    kind: kind.clone(),
                },
                Vec::new(),
            ),
            // The cheapest projection in the module: two `Arc` clones and a
            // string. The expensive half of a page's card happened on another
            // thread two seconds ago.
            SeatSource::Page { picture } => (
                MiniSeatContent::Page {
                    key: picture.key.clone(),
                    rgba: std::sync::Arc::clone(&picture.rgba),
                    width_px: picture.width_px,
                    height_px: picture.height_px,
                },
                Vec::new(),
            ),
        }
    }
}

/// **Which face one mini seat's content is set in**, as the numbers a row is
/// measured and drawn with.
///
/// One table for three readers — the row count below, the cut above and the
/// painter (`seats::focus_mini_seat_content`) — so that a seat cannot be
/// *measured* at the terminal's line height and *drawn* at the window's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiniMetrics {
    pub font_logical_px: f32,
    pub line_height: f32,
    /// The terminal's face rather than the window's — `ChromeLabel::mono`.
    pub mono: bool,
}

impl MiniMetrics {
    /// The terminal's: a shell's tail, and a document that is code.
    pub const TERM: Self = Self {
        font_logical_px: FOCUS_MINI_TERM_FONT_LOGICAL_PX,
        line_height: FOCUS_MINI_TERM_LINE_HEIGHT,
        mono: true,
    };
    /// The window's: a files column's rows, prose, and a face's two words.
    pub const FACE: Self = Self {
        font_logical_px: FOCUS_MINI_FILES_FONT_LOGICAL_PX,
        line_height: FOCUS_MINI_FILES_LINE_HEIGHT,
        mono: false,
    };

    /// One row's height in physical pixels, rounded exactly the way the painter
    /// rounds it — the same expression in one place rather than two that agree
    /// until somebody changes one.
    #[must_use]
    pub fn line_px(self, scale: f32) -> f32 {
        (self.font_logical_px * self.line_height * scale).round()
    }
}

impl SeatSource<'_> {
    /// Which of the two faces this seat's content is set in.
    ///
    /// A face's own two lines answer `FACE` for the reason the painter gives:
    /// they are the window talking about a pane, not a document quoted out of
    /// one.
    #[must_use]
    pub fn metrics(&self) -> MiniMetrics {
        match self {
            Self::Terminal { .. } | Self::Document { mono: true, .. } => MiniMetrics::TERM,
            // A picture has no rows and no columns, and the answer it gives here
            // is only ever spent on the two counts it does not use. The window's
            // face, because that is what every seat this module cannot quote
            // answers — and because a page whose picture is dropped becomes a
            // `Face`, which must not change its own measurements on the way.
            Self::Files { .. }
            | Self::Document { mono: false, .. }
            | Self::Face { .. }
            | Self::Page { .. } => MiniMetrics::FACE,
        }
    }
}

/// **How many whole rows fit down one mini seat** (user ruling, 2026-08-20) —
/// the number that replaced `FOCUS_MINI_TERM_ROWS = 6`.
///
/// The seat's own rectangle, less its hairline and the padding its rows are set
/// inside, divided by one row's height. `line` is already scaled — see
/// [`MiniMetrics::line_px`] — because the painter lays its rows out on that very
/// number, and a count derived from an unrounded height would disagree with the
/// boxes actually drawn.
///
/// **Floored, where [`mini_columns`] adds one.** The two edges are not
/// symmetrical: a character straddling the right edge is drawn as far as the box
/// goes and reads as *cut*, which is what a line running off the side of a
/// terminal looks like; a row straddling the top edge is a band of half-glyphs,
/// which reads as damage. So the horizontal edge takes the extra unit and the
/// vertical one does not.
#[must_use]
pub fn mini_rows(rect: [f32; 4], line: f32, scale: f32) -> usize {
    let border = (bt_render::FOCUS_MINI_BORDER_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let pad_top = (bt_render::FOCUS_MINI_ROW_PADDING_TOP_LOGICAL_PX * scale).round();
    let pad_bottom = (bt_render::FOCUS_MINI_ROW_PADDING_BOTTOM_LOGICAL_PX * scale).round();
    let height = rect[3] - rect[1] - 2.0 * border - pad_top - pad_bottom;
    // A line height of zero — a face nobody has measured, a scale of nothing —
    // answers "no rows" rather than dividing, for [`mini_columns`]'s reason.
    if height <= 0.0 || line <= 0.0 || !line.is_finite() {
        return 0;
    }
    (height / line) as usize
}

/// **How many columns fit across one mini seat** — §3.3's second width, as
/// the number a session's tail is cut to.
///
/// `advance` is one character of the seat's own face, measured by the renderer
/// (`WindowRuntime::focus_mini_advance` for the terminal's, `focus_mini_face_advance`
/// for the window's).
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

/// The last `rows` rows of a session's screen, oldest first, each cut to
/// `columns` **columns**.
///
/// # What is kept, and what is skipped
///
/// **A blank row inside the tail is kept, and keeps its place.** A real terminal
/// prints blank lines on purpose — the gap between paragraphs, the empty line
/// above a prompt — and a projection that closed those gaps would be a picture
/// that does not line up with the screen it is a picture of. The bottom-up walk
/// pushes every row it sees once it has seen a non-blank one, so those gaps
/// arrive as empty strings and the painter spends a row's height on each.
///
/// **The blank *floor* is skipped, and that is the difference between a
/// thumbnail and a picture of the bottom of a rectangle.** A shell that has not
/// filled its screen yet leaves every row under its prompt empty; the bottom
/// rows of that grid are empty rows, which is a true statement about the grid
/// and tells a reader nothing at all. So the walk starts counting at the last
/// row with something on it. The two rules are one rule seen from two ends —
/// the tail begins at the last thing the shell said, and everything from there
/// up is carried whole.
///
/// The walk is bounded by the grid and is only ever reached when the generation
/// moved, so a session with nothing happening in it does not walk at all — and a
/// session busy enough to walk far is, by the time it is, a session whose screen
/// has filled and whose walk is one row long.
fn transcript_tail(
    session: &DualPlaneSession,
    columns: usize,
    rows: usize,
    skip: usize,
) -> (Vec<String>, bool) {
    if rows == 0 {
        return (Vec::new(), false);
    }
    let (_, grid_rows) = session.live_dimensions();
    let wanted = rows.saturating_add(skip);
    let mut climb: Vec<String> = Vec::with_capacity(wanted);
    for row in (0..grid_rows.get()).rev() {
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
        if text.is_empty() && climb.is_empty() {
            continue;
        }
        climb.push(cut_to(text, columns));
        if climb.len() == wanted {
            break;
        }
    }
    // **Clamped to what is there**, which is the whole of "a window driven past
    // the top stops at the top". A seat holding fewer rows than the reader asked
    // to skip would otherwise hand back an empty picture, and an empty card is
    // the one answer a reader turning a wheel cannot tell from a broken one.
    // Note that this is also what keeps the two rulings of 2026-08-21 from
    // meeting: a screen with less on it than the seat holds clamps `skip` to
    // zero, so the top-aligned short tail is exactly the `skip == 0` case and
    // nothing here can produce a short list with rows hidden under it.
    let aimed = skip.min(climb.len().saturating_sub(rows));
    let window: Vec<String> = climb.into_iter().skip(aimed).take(rows).rev().collect();
    (window, aimed > 0)
}

/// `text`, cut to `columns` **drawn columns** — leading whitespace and all.
///
/// **Columns and not characters** (user ruling, 2026-08-20). The count this is
/// measured against came out of a division by a monospaced cell's advance
/// ([`mini_columns`]), and a cell is what [`bt_unicode`] calls a column: a CJK
/// ideograph occupies two of them and a `char` count says one, so a line with
/// any wide text in it used to be cut a character-for-column too late and ran
/// out past the seat's edge under the clip. Counting the way the grid the text
/// came out of counts is what makes the seat's right edge the *same* edge the
/// terminal's would be.
///
/// **Nothing is trimmed off the front.** A row's indent is its position in the
/// grid, and a projection that left-aligned every row would hand back a picture
/// with every table, every tree and every centred banner flattened against the
/// left margin. The row is written from column zero and clipped at the seat's
/// right edge, which is exactly what the terminal under it does.
///
/// The cluster straddling the edge is kept for [`mini_columns`]'s reason: the
/// painter's clip is what makes a cut line look cut.
fn cut_to(text: &str, columns: usize) -> String {
    let mut used = 0;
    let mut end = 0;
    for cluster in bt_unicode::graphemes(text) {
        if used >= columns {
            return text[..end].to_owned();
        }
        used += bt_unicode::cluster_width(cluster);
        end += cluster.len();
    }
    text.to_owned()
}

/// **What a column standing on its Git page says**: the place, and the page
/// (`docs/DESIGN.md` §7.1.6b′ F2, 2026-08-20).
///
/// # Why a face and not that page's own first rows
///
/// F2's rule is that anything with rows to count gets drawn as rows, and the
/// face is for documents that shrink to a grey smear. The Git page looks like it
/// has rows. It gets a face anyway, and for three reasons that are about this
/// product rather than about typography:
///
/// 1. **Its rows are not in memory for the cards that would draw them.** R31 is
///    that a repository is read *for a surface that is looking at it*: the walk
///    that asks git anything only ever visits the active tab's on-screen columns
///    (`columns_wanting_git`). Every card in the focus column except one is a
///    **background** tab, whose [`crate::git::GitCache`] is therefore empty, and
///    `git_panel::build` over an empty cache answers one sentence — *Reading the
///    repository…* — forever. A column of cards all saying that would be a
///    picture of nothing; the only way to make it a picture of something is to
///    start a subprocess per card, which is the one thing
///    [`files_head`] is written not to do and which no gate in this module could
///    refuse afterwards.
/// 2. **The mini row is the *tree's* row.** [`MiniFilesRow`] carries a depth to
///    indent by and a folder-or-file mark, because that is what a tree row is.
///    The Git page's rows are, in its own words, six kinds with no identity in
///    common — a masthead, headings, changes, branches, commits — none of which
///    has a depth and none of which is a file or a folder. Pushing them through
///    this row would put `#i-file` beside the word *BRANCHES*, which is the card
///    speaking a dialect: the thing §7.1.6b′ forbids in the same breath as it
///    forbids a second vocabulary.
/// 3. **The house already draws this surface as a face.** The commit-graph
///    document's seat is a face reading *Graph* today, and a Git page is that
///    same repository seen from the same place — [`FilesView`]'s own note is
///    that a files pane is a *place's* view and the repository is the same place
///    seen another way. One answer for both is this product's grammar, not a new
///    one.
///
/// Both words come from functions the pane itself prints from, which is the rest
/// of §7.1.6b′'s rule: the place is the column's root through
/// [`crate::profiles::cwd_leaf`] — the leaf rule a pane head, a tab title and a
/// Recent row all wear, and which that function's own note already binds to
/// `main.rs`'s spelling of it — and the page is [`FilesView::label`], the very
/// word on the switch the reader pressed, translated with it.
///
/// A column with nowhere to stand falls through to its kind's own word for the
/// reason a rootless head does, and on the same predicate the tree uses for its
/// *unrooted* notice ([`files::root_is_addressable`]), so the card and the pane
/// cannot disagree about whether this column is anywhere.
fn git_face(state: &FilesLeafState) -> MiniSeatContent {
    MiniSeatContent::Face {
        name: if files::root_is_addressable(&state.root) {
            crate::profiles::cwd_leaf(&state.root).to_owned()
        } else {
            seat_title(SeatKind::Files).to_owned()
        },
        kind: FilesView::Git.label().to_owned(),
    }
}

/// The first `rows` rows a files column is showing **on its tree page** — see
/// [`git_face`] for the other one.
///
/// The column's **own** walk (`files::tree_view`), so a card can never show a
/// tree the pane under it disagrees with — and **it still asks nobody
/// anything**: what comes back beside the rows is `tree_view`'s own list of the
/// directories it could not answer, handed up to the caller that owns the
/// worker. A thumbnail does not put a question to the disk; it says which
/// question there is (user ruling 2026-08-21, [`UnreadDir`]).
///
/// Dropping that list on the floor was right for as long as a card was the only
/// picture of a tab nobody was looking at. §7.1.6b′ changed what "on screen"
/// means: the column of cards puts every background tab's seats on the glass,
/// and a `Loading…` row for a read nobody had started is a card promising
/// something that was never coming.
///
/// A folder nobody has opened still stays a folder nobody has opened —
/// `tree_view` walks the open set and nothing else, so what is asked for is
/// exactly what the card is already drawing a placeholder line for.
fn files_head(
    state: &FilesLeafState,
    cache: &DirCache,
    rows: usize,
) -> (Vec<MiniFilesRow>, Vec<String>) {
    let view = files::tree_view(state, cache);
    let head = view
        .rows
        .into_iter()
        .take(rows)
        .map(|row| MiniFilesRow {
            directory: matches!(row.kind, RowKind::Directory { .. }),
            depth: u16::try_from(row.depth).unwrap_or(u16::MAX),
            name: row.name,
        })
        .collect();
    (head, view.wanted)
}

/// **The first `rows` lines of a preview's body** (user ruling, 2026-08-20).
///
/// The head of the head. [`PreviewBuffer::content`] is already only the first
/// `PREVIEW_HEAD_BYTES` of the file — a preview *is* a head read — so what this
/// takes is the top of a body that is in memory, and there is nothing behind it
/// to go and ask. That is the whole of how the ruling was satisfied without
/// crossing §7.1.6b′'s red line: **a thumbnail must not put a question to the
/// disk**, and a `take(rows)` over a `String` this window already holds asks
/// nobody anything.
///
/// **The head and not the scrolled view**, deliberately. A card is a picture of
/// *which tab this is*, and the answer to that question is at the top of the
/// document; following a pane's scroll would mean re-deriving the pane's own
/// body layout (a rendered markdown page has no line-to-pixel mapping at all)
/// inside a projection whose entire budget is four gates and a walk over
/// strings. The files column beside it already answers the same way — the top
/// of its tree, not the top of its scrollport — so this is the house's existing
/// grammar rather than a second one.
///
/// A mono document is cut like a transcript row; prose is cut at the same count
/// measured against the app face's own advance, so that in both cases the
/// painter's clip has at most one column's worth of text left to bite off.
fn document_head(buffer: &PreviewBuffer, columns: usize, rows: usize) -> Vec<String> {
    buffer
        .content
        .as_deref()
        .unwrap_or_default()
        .lines()
        .take(rows)
        // A `\r\n` file read as text leaves the carriage return on the end of
        // every line, and a chrome label handed one shapes a box for a glyph
        // nobody asked for.
        .map(|line| cut_to(line.trim_end_matches('\r'), columns))
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
            rows: 6,
            source: SeatSource::Face {
                name: name.to_owned(),
                kind: "TXT".to_owned(),
            },
        }
    }

    /// A files column rooted somewhere real, with its root directory already
    /// read — a tree with something in it to draw.
    fn files_column(view: FilesView) -> (FilesLeafState, DirCache) {
        let mut cache = DirCache::default();
        cache.accept(
            "",
            files::DirOutcome::Listed(files::DirListing {
                entries: vec![
                    files::DirEntry {
                        name: "src".to_owned(),
                        is_dir: true,
                        is_symlink: false,
                    },
                    files::DirEntry {
                        name: "Cargo.toml".to_owned(),
                        is_dir: false,
                        is_symlink: false,
                    },
                ],
                omitted: 0,
                canonical: None,
            }),
        );
        (
            FilesLeafState {
                root: r"D:\Developer\folio".to_owned(),
                view,
                ..FilesLeafState::default()
            },
            cache,
        )
    }

    fn files_demand<'a>(state: &'a FilesLeafState, cache: &'a DirCache) -> SeatDemand<'a> {
        SeatDemand {
            id: seat(1),
            columns: 24,
            rows: 4,
            source: SeatSource::Files { state, cache },
        }
    }

    /// A file already read into a buffer — a preview seat with something to
    /// quote. `PREVIEW_HEAD_BYTES` is not involved: this is a body the window is
    /// already holding, which is the only kind a thumbnail is allowed to read.
    fn loaded_buffer(name: &str, body: &str) -> PreviewBuffer {
        let mut buffer = PreviewBuffer::new(
            PreviewSource::File(std::path::PathBuf::from(format!(
                r"D:\Developer\folio\{name}"
            ))),
            name.to_owned(),
        );
        buffer.accept(crate::preview::HeadOutcome::Read {
            text: body.to_owned(),
            truncated: false,
            mtime: None,
        });
        buffer
    }

    fn document_demand(buffer: &PreviewBuffer, columns: usize, rows: usize) -> SeatDemand<'_> {
        SeatDemand {
            id: seat(1),
            columns,
            rows,
            source: SeatSource::Document { buffer, mono: true },
        }
    }

    fn shown(thumbs: &FocusThumbnails) -> Option<&MiniSeatContent> {
        thumbs.seats(tab(1)).and_then(|seats| seats.get(&seat(1)))
    }

    /// What a column on its Git page says: the place, and the page it is on.
    ///
    /// Spelled out here rather than called from [`git_face`], so that the tests
    /// assert against a written-down expectation and not against the code that
    /// produced it.
    fn git_face_saying(place: &str) -> MiniSeatContent {
        MiniSeatContent::Face {
            name: place.to_owned(),
            kind: FilesView::Git.label().to_owned(),
        }
    }

    /// **A column standing on its Git page draws the Git page** (§7.1.6b′ F2,
    /// 2026-08-20) — not the tree it would draw on its other page.
    ///
    /// A card is a small picture of a tab *as it is now*, and `FilesLeafState`'s
    /// `view` is what decides which of a column's two pages that is — the same
    /// bit §7.1.3g made the keyboard follow. A card that answers with the tree
    /// whatever the column is showing is a card saying something the seat is not.
    #[test]
    fn a_column_on_its_git_page_does_not_draw_the_tree_it_is_not_showing() {
        let (state, cache) = files_column(FilesView::Git);
        let mut thumbs = FocusThumbnails::default();
        thumbs.project(tab(1), &[files_demand(&state, &cache)], Instant::now());
        assert_eq!(shown(&thumbs), Some(&git_face_saying("folio")));
    }

    /// And the other page still draws the tree, which is the half that was never
    /// wrong.
    #[test]
    fn a_column_on_its_tree_still_draws_the_tree() {
        let (state, cache) = files_column(FilesView::Files);
        let mut thumbs = FocusThumbnails::default();
        thumbs.project(tab(1), &[files_demand(&state, &cache)], Instant::now());
        assert_eq!(
            shown(&thumbs),
            Some(&MiniSeatContent::Files(vec![
                MiniFilesRow {
                    name: "src".to_owned(),
                    depth: 0,
                    directory: true,
                },
                MiniFilesRow {
                    name: "Cargo.toml".to_owned(),
                    depth: 0,
                    directory: false,
                },
            ]))
        );
    }

    /// **A card is a reason to read the directory under it** (user ruling
    /// 2026-08-21) — the projection quotes what is in memory, and says out loud
    /// what it had nothing in memory for.
    ///
    /// A background tab's files column has never been walked: `files_trees`
    /// walks the *active* tab and no other, which was right for as long as a
    /// background tab was off screen. The focus column put it on screen, and
    /// what the card then drew was `📄 Loading…` — a `RowNotice::Loading` for a
    /// read nobody had started and nobody was going to start, which is the
    /// user's screenshot.
    ///
    /// The projection still asks nobody anything. It hands the caller the keys
    /// [`files::tree_view`] could not answer — the very list the docked walk
    /// already hands `Runtime::files_trees` — and the caller, who owns the
    /// worker, decides whether to go.
    ///
    /// MUTATION: throw `TreeView::wanted` away here — the card sits on
    /// "Loading…" until somebody clicks the tab.
    #[test]
    fn a_card_whose_column_was_never_walked_asks_for_the_directory_it_could_not_draw() {
        let state = FilesLeafState {
            root: r"D:\Developer\folio".to_owned(),
            view: FilesView::Files,
            ..FilesLeafState::default()
        };
        let cache = DirCache::default();
        let mut thumbs = FocusThumbnails::default();
        let asks = thumbs.project(tab(1), &[files_demand(&state, &cache)], Instant::now());
        assert_eq!(
            asks,
            vec![UnreadDir {
                seat: seat(1),
                key: String::new(),
            }],
            "the root is what a column with an empty cache could not draw"
        );
    }

    /// **And it is asked once.** The ledger is the one the docked walk already
    /// keeps — [`DirCache`]'s own `Pending` — so a card looked at sixty times a
    /// second reads the directory on the first of those frames and on none of
    /// the rest.
    ///
    /// The mark is made by the caller, exactly as `files_trees` makes it, and
    /// this test plays that caller: ask, mark, then keep projecting.
    ///
    /// MUTATION: ask without marking — a card becomes a `read_dir` at 60 Hz.
    #[test]
    fn a_directory_already_asked_for_is_asked_no_second_time() {
        let state = FilesLeafState {
            root: r"D:\Developer\folio".to_owned(),
            view: FilesView::Files,
            ..FilesLeafState::default()
        };
        let mut cache = DirCache::default();
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        let asks = thumbs.project(tab(1), &[files_demand(&state, &cache)], start);
        assert_eq!(asks.len(), 1, "the first frame asks");
        for ask in &asks {
            cache.mark_pending(&ask.key);
        }
        for frame in 1..20 {
            assert!(
                thumbs
                    .project(
                        tab(1),
                        &[files_demand(&state, &cache)],
                        start + MIN_INTERVAL * frame
                    )
                    .is_empty(),
                "a question already asked is not asked again"
            );
        }
    }

    /// A column whose listing is in memory asks for nothing at all — the gate is
    /// "what could this card not draw", never "is this card a files column".
    #[test]
    fn a_card_whose_column_has_its_listing_asks_for_nothing() {
        let (state, cache) = files_column(FilesView::Files);
        let mut thumbs = FocusThumbnails::default();
        assert!(
            thumbs
                .project(tab(1), &[files_demand(&state, &cache)], Instant::now())
                .is_empty()
        );
    }

    /// **Gate 3 has to carry the page too.** Turning a column over repaints its
    /// card — otherwise the picture is right the first time and a lie ever after,
    /// which is the same bug wearing the throttle's clothes.
    ///
    /// The two assertions are both needed: the counter says the gate let the work
    /// through, and the content says the work drew the other page. A key that
    /// changed and a projection that did not would pass the first alone.
    #[test]
    fn turning_a_column_to_its_git_page_and_back_repaints_its_card() {
        let (tree, cache) = files_column(FilesView::Files);
        let (git, _) = files_column(FilesView::Git);
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[files_demand(&tree, &cache)], start);
        assert_eq!(thumbs.stats().projections, 1);
        assert!(matches!(shown(&thumbs), Some(MiniSeatContent::Files(_))));

        thumbs.project(tab(1), &[files_demand(&git, &cache)], start + MIN_INTERVAL);
        assert_eq!(
            thumbs.stats().projections,
            2,
            "the page the column is on is part of what its card is built from"
        );
        assert_eq!(shown(&thumbs), Some(&git_face_saying("folio")));

        thumbs.project(
            tab(1),
            &[files_demand(&tree, &cache)],
            start + MIN_INTERVAL * 2,
        );
        assert_eq!(thumbs.stats().projections, 3);
        assert!(matches!(shown(&thumbs), Some(MiniSeatContent::Files(_))));
    }

    /// A column with nowhere to stand is named the way its own head names it —
    /// the kind's word — rather than with an empty line where a place should be.
    #[test]
    fn a_rootless_column_on_its_git_page_wears_its_kinds_own_word() {
        let mut thumbs = FocusThumbnails::default();
        let state = FilesLeafState {
            view: FilesView::Git,
            ..FilesLeafState::default()
        };
        thumbs.project(
            tab(1),
            &[files_demand(&state, &files::EMPTY_DIR_CACHE)],
            Instant::now(),
        );
        assert_eq!(
            shown(&thumbs),
            Some(&git_face_saying(seat_title(SeatKind::Files)))
        );
    }

    /// **The idle claim survives the page** — a column nobody is touching is
    /// still refused on one comparison, on whichever of its two pages it is
    /// standing.
    #[test]
    fn an_idle_column_on_its_git_page_projects_once_and_then_never_again() {
        let (state, cache) = files_column(FilesView::Git);
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        for frame in 0..=50 {
            thumbs.project(
                tab(1),
                &[files_demand(&state, &cache)],
                start + MIN_INTERVAL * frame,
            );
        }
        let stats = thumbs.stats();
        assert_eq!(stats.projections, 1);
        assert_eq!(stats.skipped_unchanged, 50);
        assert_eq!(stats.skipped_throttled, 0);
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

    /// **The gesture channel** — a seat the hand has just re-aimed rebuilds on
    /// the spot, however recently the clock last let it through.
    ///
    /// Gate 4's whole argument is written against a *shell* writing faster than
    /// a 160px card can show; it says nothing about a hand, and a hand turning
    /// the wheel twice inside a tenth of a second is a hand that is owed two
    /// pictures. Ten aims in a hundred milliseconds is ten projections of **one**
    /// seat, which is what makes this an exception and not a hole: the credit is
    /// spent per seat, by the seat the pointer was over.
    #[test]
    fn a_seat_the_hand_just_re_aimed_rebuilds_inside_the_clock() {
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[face(1, "aim 0")], start);
        for tick in 1..=10 {
            thumbs.unthrottle(tab(1), seat(1));
            thumbs.project(
                tab(1),
                &[face(1, &format!("aim {tick}"))],
                start + Duration::from_millis(10 * tick),
            );
        }
        let stats = thumbs.stats();
        assert_eq!(
            stats.projections, 11,
            "every turn of the wheel got the picture it asked for"
        );
        assert_eq!(
            stats.skipped_throttled, 0,
            "the clock never refused a gesture"
        );
        assert_eq!(
            thumbs.seats(tab(1)).and_then(|seats| seats.get(&seat(1))),
            Some(&MiniSeatContent::Face {
                name: "aim 10".to_owned(),
                kind: "TXT".to_owned(),
            }),
            "and the card is showing where it was last aimed, not where it was aimed first"
        );
    }

    /// The gesture channel is **the clock's exception and not damage's**: an aim
    /// that changed nothing behind the seat still rebuilds nothing.
    #[test]
    fn a_gesture_that_left_the_seat_saying_the_same_thing_rebuilds_nothing() {
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[face(1, "a")], start);
        thumbs.unthrottle(tab(1), seat(1));
        thumbs.project(tab(1), &[face(1, "a")], start + Duration::from_millis(10));
        let stats = thumbs.stats();
        assert_eq!(stats.projections, 1);
        assert_eq!(stats.skipped_unchanged, 1, "gate 3 still refused it");
        assert_eq!(stats.skipped_throttled, 0);
    }

    /// And the credit is spent when it is looked at: one aim buys one projection,
    /// not a seat that has stopped answering to the clock at all.
    #[test]
    fn the_gesture_credit_is_spent_by_the_pass_that_uses_it() {
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[face(1, "a")], start);
        thumbs.unthrottle(tab(1), seat(1));
        thumbs.project(tab(1), &[face(1, "b")], start + Duration::from_millis(10));
        thumbs.project(tab(1), &[face(1, "c")], start + Duration::from_millis(20));
        let stats = thumbs.stats();
        assert_eq!(
            stats.projections, 2,
            "the shell's next line waited its turn"
        );
        assert_eq!(stats.skipped_throttled, 1);
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
                rows: 6,
                source: SeatSource::Terminal {
                    session: shell,
                    skip: 0,
                },
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
            rows: 6,
            source: SeatSource::Terminal {
                session: &shell,
                skip: 0,
            },
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
            transcript_tail(&shell, 40, 6, 0).0,
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
            transcript_tail(&shell, 40, 6, 0).0,
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
        assert_eq!(
            transcript_tail(&shell, 8, 6, 0).0,
            vec!["abcdefgh".to_owned()]
        );
    }

    /// A cut that landed inside a character would not be a string at all — and,
    /// since 2026-08-20, it is counted in the unit the grid counts in.
    ///
    /// Three columns of a line of ideographs is one whole ideograph and the one
    /// straddling the edge, which is two of them: the same "draw it and let the
    /// clip cut it" rule [`mini_columns`] adds its extra column for. It used to
    /// answer `日本語` — three *characters*, six columns, half a card's width
    /// past the edge.
    #[test]
    fn the_cut_counts_columns_and_never_bytes() {
        assert_eq!(cut_to("日本語のテキスト", 3), "日本");
        assert_eq!(cut_to("日本語のテキスト", 4), "日本");
        assert_eq!(cut_to("日本語のテキスト", 5), "日本語");
        assert_eq!(cut_to("ascii", 99), "ascii");
        // A grapheme is one unit however many code points went into it.
        assert_eq!(cut_to("e\u{301}xyz", 2), "e\u{301}x");
    }

    /// **A row keeps its columns** (user ruling, 2026-08-20).
    ///
    /// Three claims, and the middle one is the one that was wrong:
    ///
    /// * an indent is kept — a row is written from column zero, so the spaces in
    ///   front of it are drawn and the shape of a tree, a table or a centred
    ///   banner survives the shrink;
    /// * the cut is measured in **columns** — a CJK ideograph is two of them,
    ///   which is what the grid the text came out of counted it as, and counting
    ///   `char`s instead put the right edge a character-per-ideograph too far
    ///   right on every line with any wide text in it;
    /// * and it is cut at the **right** edge, never from the left.
    ///
    /// Red gate: count characters instead of columns and the second assertion
    /// goes red with `"中文ab"`, which is what it did before this branch.
    #[test]
    fn a_row_keeps_its_columns() {
        assert_eq!(cut_to("    indented", 40), "    indented");
        assert_eq!(cut_to("中文abc", 4), "中文");
        assert_eq!(cut_to("abcdefgh", 4), "abcd");
        // A row narrower than the seat is left entirely alone — no padding, and
        // nothing taken off either end.
        assert_eq!(cut_to("  ok", 40), "  ok");
    }

    /// And through a real grid, which is where the indent actually comes from: a
    /// shell that printed a line four columns in has a card that says so.
    #[test]
    fn an_indented_row_is_still_indented_on_the_card() {
        let mut shell = session();
        shell
            .feed(b"    indented\r\n")
            .expect("a shell takes its own output");
        assert_eq!(
            transcript_tail(&shell, 40, 6, 0).0,
            vec!["    indented".to_owned()]
        );
    }

    /// **A blank row inside the tail keeps its place** (user ruling,
    /// 2026-08-20) — the gap between two paragraphs and the empty line above a
    /// prompt are things the shell printed on purpose, and a projection that
    /// closed them would not line up with the screen it is a picture of.
    ///
    /// This one was **already true** when the ruling was written, and the guard
    /// is what the branch adds: the walk skips blanks only while it is still
    /// under the floor. What was wrong was the doc comment above it, which said
    /// "the last N rows that have anything on them".
    #[test]
    fn blank_rows_inside_the_tail_are_kept() {
        let mut shell = session();
        shell
            .feed(b"one\r\n\r\ntwo\r\n")
            .expect("a shell takes its own output");
        assert_eq!(
            transcript_tail(&shell, 40, 6, 0).0,
            vec!["one".to_owned(), String::new(), "two".to_owned()]
        );
    }

    /// **A taller seat projects more rows** (user ruling, 2026-08-20) — the
    /// count is the seat's own inner height over its own line height, floored,
    /// and never a constant.
    ///
    /// The two halves are both needed: the first says the arithmetic is the
    /// stated one at three scales, the second says the projection actually
    /// spends it. A `mini_rows` that answered thirteen while `transcript_tail`
    /// went on taking six would pass the first alone.
    ///
    /// Red gate: put `FOCUS_MINI_TERM_ROWS = 6` back and the second half goes
    /// red with six lines out of a shell that has thirty.
    #[test]
    fn a_tall_seat_projects_as_many_rows_as_it_holds() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let line = MiniMetrics::TERM.line_px(scale);
            let border = (bt_render::FOCUS_MINI_BORDER_LOGICAL_PX * scale)
                .round()
                .max(1.0);
            let pad = (bt_render::FOCUS_MINI_ROW_PADDING_TOP_LOGICAL_PX * scale).round()
                + (bt_render::FOCUS_MINI_ROW_PADDING_BOTTOM_LOGICAL_PX * scale).round();
            // A lone pane's seat: the whole card body less the body's own inset.
            let height = (bt_render::DEFAULT_FOCUS_MINI_HEIGHT_LOGICAL_PX * scale).round()
                - 2.0 * (bt_render::FOCUS_MINI_PADDING_LOGICAL_PX * scale).round();
            let rect = [0.0, 0.0, 263.0 * scale, height];
            let held = mini_rows(rect, line, scale);
            assert_eq!(
                held,
                ((height - 2.0 * border - pad) / line) as usize,
                "the count is the inner height over the line height, floored"
            );
            assert!(
                held > 6,
                "a 160px card holds more than the six a constant used to allow \
                 (scale {scale}: {held})"
            );
        }

        // And the projection spends what it was given.
        let mut shell = DualPlaneSession::new(
            NonZeroU32::new(80).expect("80 is not zero"),
            NonZeroU32::new(40).expect("40 is not zero"),
        );
        for line in 1..=30 {
            shell
                .feed(format!("line {line}\r\n").as_bytes())
                .expect("a shell takes its own output");
        }
        assert_eq!(transcript_tail(&shell, 60, 13, 0).0.len(), 13);
        assert_eq!(
            transcript_tail(&shell, 60, 13, 0)
                .0
                .first()
                .map(String::as_str),
            Some("line 18"),
            "and they are the LAST thirteen, in reading order"
        );
    }

    /// A shell with `lines` numbered lines printed onto a forty-row screen, which
    /// is what an aimed window is measured against: enough behind the tail for
    /// the window to be lifted off it, and a real floor to run out of.
    fn talking_shell(lines: u32) -> DualPlaneSession {
        let mut shell = DualPlaneSession::new(
            NonZeroU32::new(80).expect("80 is not zero"),
            NonZeroU32::new(40).expect("40 is not zero"),
        );
        for line in 1..=lines {
            shell
                .feed(format!("line {line}\r\n").as_bytes())
                .expect("a shell takes its own output");
        }
        shell
    }

    /// **An aimed window is the tail lifted by exactly the rows it was given**
    /// (user ruling, 2026-08-21 — §7.1.6b′ 「卡片窗口瞄准」).
    ///
    /// The case that produced the ruling, in the smallest form that shows it: a
    /// program whose bottom thirteen rows are furniture puts its newest real
    /// output thirteen rows above the floor, so a window lifted thirteen rows is
    /// pointed at it and stays pointed at it as the program goes on printing.
    ///
    /// **Zero is the tail and is the same picture it always was**, which is what
    /// makes this a window and not a rewrite: the first half of the assertion is
    /// the shipped behaviour, unchanged.
    ///
    /// Red gate: ignore `skip` in `transcript_tail` and the aimed half comes back
    /// showing the last thirteen lines — the furniture — while the unaimed half
    /// goes on passing.
    #[test]
    fn an_aimed_window_is_the_tail_lifted_by_the_rows_it_was_given() {
        let shell = talking_shell(40);

        let (tail, more_below) = transcript_tail(&shell, 60, 13, 0);
        assert_eq!(tail.first().map(String::as_str), Some("line 28"));
        assert_eq!(tail.last().map(String::as_str), Some("line 40"));
        assert!(
            !more_below,
            "a window on the tail has nothing under it, and says so"
        );

        let (aimed, more_below) = transcript_tail(&shell, 60, 13, 13);
        assert_eq!(
            aimed.first().map(String::as_str),
            Some("line 15"),
            "lifted thirteen rows, the window begins thirteen rows higher"
        );
        assert_eq!(
            aimed.last().map(String::as_str),
            Some("line 27"),
            "and ends thirteen rows above where the tail ended"
        );
        assert_eq!(aimed.len(), 13, "the seat is as full as it ever was");
        assert!(
            more_below,
            "with the thirteen rows it lifted off still under it"
        );
    }

    /// A window driven past the top of what is on the screen **stops at the
    /// top** rather than running off it (user ruling, 2026-08-21).
    ///
    /// An empty card is the one answer a reader turning a wheel cannot tell from
    /// a broken one, so the aim is clamped to what there is and the picture goes
    /// on being a picture.
    ///
    /// Red gate: drop the `min` in `transcript_tail` and this comes back empty.
    #[test]
    fn a_window_driven_past_the_top_stops_at_the_top() {
        let shell = talking_shell(20);
        let (window, more_below) = transcript_tail(&shell, 60, 13, 1_000);
        assert_eq!(window.len(), 13, "the seat is still full");
        assert_eq!(
            window.first().map(String::as_str),
            Some("line 1"),
            "and what it holds is the top of what the screen has on it"
        );
        assert!(
            more_below,
            "with the seven it is not showing still underneath"
        );
    }

    /// **The two rulings of 2026-08-21 cannot meet** — aiming a seat whose screen
    /// holds less than the seat does changes nothing, so the top-aligned short
    /// tail is always the unaimed case.
    ///
    /// Stated here rather than left to follow from the clamp because it is the
    /// question a reader of either ruling asks about the other: what does a
    /// two-line shell do when somebody turns the wheel over it? It stays put,
    /// and it reports nothing hidden — so the painter draws two rows at the top
    /// of the cell and no seam.
    #[test]
    fn aiming_a_screen_with_less_on_it_than_the_seat_holds_changes_nothing() {
        let shell = talking_shell(2);
        let resting = transcript_tail(&shell, 60, 13, 0);
        let aimed = transcript_tail(&shell, 60, 13, 5);
        assert_eq!(
            aimed.0,
            vec!["line 1".to_owned(), "line 2".to_owned()],
            "both lines, in reading order"
        );
        assert_eq!(aimed, resting, "and aiming it did not move a thing");
        assert!(!aimed.1, "there is nothing below to draw a seam for");
    }

    /// **The aim is in the damage key**, so turning the wheel re-projects the
    /// seat it was turned over (user ruling, 2026-08-21).
    ///
    /// Gate 3 is what makes an idle window free, and it refuses on "nothing
    /// behind this seat has moved" — which is true of a seat whose reader has
    /// just aimed it somewhere else. Without the aim in the key the card would
    /// go on showing the furniture until the shell happened to print.
    ///
    /// Red gate: take `skip` out of `Damage::Grid` and the second projection is
    /// skipped as unchanged.
    #[test]
    fn aiming_a_seat_re_projects_it() {
        let shell = talking_shell(40);
        let mut thumbs = FocusThumbnails::default();
        let now = Instant::now();
        let demand = |skip| SeatDemand {
            id: SeatId(1),
            columns: 60,
            rows: 13,
            source: SeatSource::Terminal {
                session: &shell,
                skip,
            },
        };
        thumbs.project(TabId(1), &[demand(0)], now);
        assert_eq!(thumbs.stats().projections, 1);
        thumbs.project(TabId(1), &[demand(0)], now + MIN_INTERVAL);
        assert_eq!(
            thumbs.stats().skipped_unchanged,
            1,
            "the same aim over the same screen is the same picture"
        );
        thumbs.project(TabId(1), &[demand(13)], now + MIN_INTERVAL * 2);
        assert_eq!(
            thumbs.stats().projections,
            2,
            "and a new aim is new work, however still the shell is"
        );
        assert!(matches!(
            thumbs
                .seats(TabId(1))
                .and_then(|seats| seats.get(&SeatId(1))),
            Some(MiniSeatContent::Transcript {
                more_below: true,
                ..
            })
        ));
    }

    /// A seat with no room for a whole row asks for none, rather than for one it
    /// would draw as a band of half-glyphs.
    #[test]
    fn a_seat_too_short_for_a_row_projects_none() {
        let line = MiniMetrics::TERM.line_px(1.0);
        assert_eq!(mini_rows([0.0, 0.0, 200.0, 10.0], line, 1.0), 0);
        assert_eq!(mini_rows([0.0, 0.0, 200.0, 100.0], 0.0, 1.0), 0);
    }

    /// **A preview with a body in memory quotes it** (user ruling, 2026-08-20):
    /// the head of the document, as many lines as the seat holds, cut to its
    /// width — not two words about it.
    ///
    /// Red gate: send a loaded preview back through `SeatSource::Face` and this
    /// goes red on the first assertion.
    #[test]
    fn a_loaded_preview_projects_its_head_lines() {
        let buffer = loaded_buffer(
            "main.rs",
            "fn main() {\n    println!(\"hi\");\n}\n\nfn other() {}\n",
        );
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[document_demand(&buffer, 40, 3)], start);
        assert_eq!(
            shown(&thumbs),
            Some(&MiniSeatContent::Document {
                lines: vec![
                    "fn main() {".to_owned(),
                    "    println!(\"hi\");".to_owned(),
                    "}".to_owned(),
                ],
                mono: true,
            }),
            "the head of the document, indent and all, as many lines as it holds"
        );

        // A taller seat quotes further down, and a blank line inside the file
        // keeps its place there exactly as it does in a shell's tail.
        thumbs.project(
            tab(1),
            &[document_demand(&buffer, 40, 5)],
            start + MIN_INTERVAL,
        );
        assert_eq!(
            shown(&thumbs),
            Some(&MiniSeatContent::Document {
                lines: vec![
                    "fn main() {".to_owned(),
                    "    println!(\"hi\");".to_owned(),
                    "}".to_owned(),
                    String::new(),
                    "fn other() {}".to_owned(),
                ],
                mono: true,
            })
        );
    }

    /// **A preview whose body has not arrived stays a face** — the red line, in
    /// a test: the projection has nothing in memory to quote and does not go
    /// looking for any.
    ///
    /// A background tab's preview pane is exactly this — `PreviewLoad::Pending`,
    /// no content — and it is the case the ruling's "只能从已在内存里的缓冲取文本"
    /// was written for.
    #[test]
    fn an_unloaded_preview_stays_a_face() {
        let buffer = PreviewBuffer::new(
            PreviewSource::File(std::path::PathBuf::from(r"D:\Developer\folio\notes.md")),
            "notes.md".to_owned(),
        );
        assert!(
            buffer.content.is_none(),
            "a buffer nobody has read has nothing to quote"
        );
        // The caller's own rule (`Runtime::mini_source`) is what turns this into
        // a face; asserted here as the fact it turns on, so the two cannot
        // disagree about what "loaded" means.
        assert_eq!(buffer.load, crate::preview::PreviewLoad::Pending);

        let mut thumbs = FocusThumbnails::default();
        thumbs.project(
            tab(1),
            &[SeatDemand {
                id: seat(1),
                columns: 40,
                rows: 12,
                source: SeatSource::Face {
                    name: "notes.md".to_owned(),
                    kind: buffer.kind_word(),
                },
            }],
            Instant::now(),
        );
        assert_eq!(
            shown(&thumbs),
            Some(&MiniSeatContent::Face {
                name: "notes.md".to_owned(),
                kind: "MD".to_owned(),
            })
        );
    }

    /// One page's last frame, as the store would hand it over.
    fn a_frame(key: &str) -> crate::web_thumb::Picture {
        crate::web_thumb::Picture {
            key: key.to_owned(),
            rgba: std::sync::Arc::from(vec![0x40; 8 * 8 * 4]),
            width_px: 8,
            height_px: 8,
        }
    }

    /// **Red gate (W2 slice ⑥): a web seat with a frame draws the frame, and a
    /// web seat without one is the face it always was.**
    ///
    /// The second half is the one that matters: `mini_source` hands `None` for a
    /// page nobody has photographed, and every arm below it then runs exactly as
    /// it did before this slice — `PreviewView::Web` has no lines to quote, so
    /// the seat is a face. That is the honest picture, not a fallback.
    #[test]
    fn a_page_with_a_frame_draws_it_and_a_page_without_one_stays_a_face() {
        let picture = a_frame("web-thumb:1:1");
        let mut thumbs = FocusThumbnails::default();
        thumbs.project(
            tab(1),
            &[SeatDemand {
                id: seat(1),
                columns: 40,
                rows: 12,
                source: SeatSource::Page { picture: &picture },
            }],
            Instant::now(),
        );
        assert_eq!(
            shown(&thumbs),
            Some(&MiniSeatContent::Page {
                key: "web-thumb:1:1".to_owned(),
                rgba: std::sync::Arc::from(vec![0x40; 8 * 8 * 4]),
                width_px: 8,
                height_px: 8,
            })
        );

        let mut unphotographed = FocusThumbnails::default();
        unphotographed.project(
            tab(1),
            &[SeatDemand {
                id: seat(1),
                columns: 40,
                rows: 12,
                source: SeatSource::Face {
                    name: "127.0.0.1:8080".to_owned(),
                    kind: "Page".to_owned(),
                },
            }],
            Instant::now(),
        );
        assert_eq!(
            shown(&unphotographed),
            Some(&MiniSeatContent::Face {
                name: "127.0.0.1:8080".to_owned(),
                kind: "Page".to_owned(),
            })
        );
    }

    /// **Red gate: gate 3 answers a page on its picture's identity and on
    /// nothing else.**
    ///
    /// The picture is a megabyte and the card is asked about it sixty times a
    /// second, so the damage key has to be a string comparison rather than the
    /// pixels. A frame that has not been replaced re-projects nothing; a
    /// replacement re-projects once.
    #[test]
    fn a_page_re_projects_when_its_frame_is_replaced_and_never_otherwise() {
        let picture = a_frame("web-thumb:1:1");
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(
            tab(1),
            &[SeatDemand {
                id: seat(1),
                columns: 40,
                rows: 12,
                source: SeatSource::Page { picture: &picture },
            }],
            start,
        );
        assert_eq!(thumbs.stats().projections, 1);
        for frame in 1..30 {
            thumbs.project(
                tab(1),
                &[SeatDemand {
                    id: seat(1),
                    columns: 40,
                    rows: 12,
                    source: SeatSource::Page { picture: &picture },
                }],
                start + MIN_INTERVAL * frame,
            );
        }
        assert_eq!(
            thumbs.stats().projections,
            1,
            "a still page re-projected its card"
        );
        assert_eq!(thumbs.stats().skipped_unchanged, 29);

        let replaced = a_frame("web-thumb:1:2");
        thumbs.project(
            tab(1),
            &[SeatDemand {
                id: seat(1),
                columns: 40,
                rows: 12,
                source: SeatSource::Page { picture: &replaced },
            }],
            start + MIN_INTERVAL * 30,
        );
        assert_eq!(
            thumbs.stats().projections,
            2,
            "a fresh frame did not reach the card"
        );
    }

    /// **Gate 3 carries the buffer's identity, not only its revision.** Two
    /// files freshly read are both at the same revision, so a key that was the
    /// counter alone would leave the first file's head on the card after the
    /// pane moved to the second.
    #[test]
    fn a_preview_that_changed_file_repaints_even_at_the_same_revision() {
        let first = loaded_buffer("a.rs", "the first file\n");
        let second = loaded_buffer("b.rs", "the second file\n");
        assert_eq!(
            first.revision, second.revision,
            "two freshly-read buffers are at the same revision, which is the trap"
        );
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        thumbs.project(tab(1), &[document_demand(&first, 40, 4)], start);
        thumbs.project(
            tab(1),
            &[document_demand(&second, 40, 4)],
            start + MIN_INTERVAL,
        );
        assert_eq!(thumbs.stats().projections, 2);
        assert_eq!(
            shown(&thumbs),
            Some(&MiniSeatContent::Document {
                lines: vec!["the second file".to_owned()],
                mono: true,
            })
        );
    }

    /// A card that got **taller** re-projects even though the grid behind it did
    /// not move — the vertical half of the §3.3 argument, and the reason `rows`
    /// is in the damage key.
    #[test]
    fn a_taller_card_re_projects_a_grid_that_did_not_change() {
        let shell = session();
        let mut thumbs = FocusThumbnails::default();
        let start = Instant::now();
        let demand = |rows| SeatDemand {
            id: seat(1),
            columns: 40,
            rows,
            source: SeatSource::Terminal {
                session: &shell,
                skip: 0,
            },
        };
        thumbs.project(tab(1), &[demand(6)], start);
        thumbs.project(tab(1), &[demand(6)], start + MIN_INTERVAL);
        assert_eq!(thumbs.stats().projections, 1);
        thumbs.project(tab(1), &[demand(13)], start + MIN_INTERVAL * 2);
        assert_eq!(thumbs.stats().projections, 2);
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
    /// **How many rows each of those seats carries** — the number the 2026-08-20
    /// ruling more than doubled, and therefore the one the budget has to be
    /// measured against again.
    ///
    /// **Twenty-seven**: what a lone pane's seat holds at the **tallest rung the
    /// `Focus card height` row offers** (320px, user ruling 2026-08-21), spent
    /// on *both* seats of all ten tabs. That is deliberately worse than the
    /// shape it stands for twice over — two seats sharing one body hold about
    /// half each, and a reader who never opens the row is on 160, where a lone
    /// seat holds twelve — so the ceiling below is defended against more than
    /// four times the work the default can be asked for.
    ///
    /// It was thirteen while 160 was the only height, and the number the doc
    /// beside it claimed rather than the number the arithmetic gives: a 160px
    /// card's lone seat holds **twelve** rows at the default face, which
    /// `seats::tests::a_taller_card_holds_more_rows_and_answers_over_all_of_itself`
    /// now asserts against the geometry instead of against a memory of it.
    const BUDGET_ROWS: usize = 27;
    /// **How many columns each of those seats is cut to** — the horizontal twin
    /// of [`BUDGET_ROWS`], and the number the 2026-08-20 re-strike of the
    /// column's width moved.
    ///
    /// Fifty-nine: what a **lone** pane's seat holds across a 280px column's
    /// card in the default face, spent on *both* seats of all ten tabs, for
    /// exactly [`BUDGET_ROWS`]'s reason — two seats sharing one body hold
    /// twenty-eight each, so the ceiling is being defended against more than
    /// twice the cut the mode can actually be asked for.
    const BUDGET_COLUMNS: usize = 59;

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
                        columns: BUDGET_COLUMNS,
                        rows: BUDGET_ROWS,
                        source: SeatSource::Terminal {
                            session: shell,
                            skip: 0,
                        },
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
                        columns: BUDGET_COLUMNS,
                        rows: BUDGET_ROWS,
                        source: SeatSource::Terminal {
                            session: shell,
                            skip: 0,
                        },
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
