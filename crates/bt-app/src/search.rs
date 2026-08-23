//! **In-pane search** — the capsule riding a terminal pane's top-right corner, the hits it lights
//! in the text, and the walk between them (`design/ui-mockup.html` 1493-1551 and 8505-8737;
//! `docs/DESIGN.md` §7.1.5d).
//!
//! # What it is, in the sentence the ruling gives it
//!
//! *"A capsule riding the pane's top-right: search is a **STAYING state**, so it overlays a corner
//! instead of taking a palette's center stage."* Everything about the shape follows from that: it
//! does not dim the window, it does not take the keyboard away from the shell unless you put the
//! caret in it, and closing it leaves the viewport exactly where the walk left it (B63 —
//! *"Esc closes with the viewport left where it is"*, which is deliberately the opposite of the
//! terminals that scroll you back to where you started).
//!
//! # One search, one pane
//!
//! The prototype's state is a single object (`srch`, mock 8515) and its element lookup is a
//! document-wide `querySelector` (8516) — a singleton, stated twice. This module keeps it: there is
//! one [`SearchState`] per window, it names one seat, and opening on a second pane closes the
//! first. That is not a simplification of a per-pane design; it is the design. The capsule is a
//! *place you are looking from*, and two of them would be two answers to "what does Enter do".
//!
//! # What is searched, and what that costs
//!
//! The transcript, **not the glyph soup** — the user bug of 2026-07-18 is written into the
//! prototype beside the scan: `m` counted 23 on a screen that was showing 8, because the DOM held
//! the same text twice over in a rendered block and its source. So the count here is a count of
//! hits in *lines*, and it cannot change with what the screen happens to be showing.
//!
//! Three planes are scanned, because a terminal's text lives in three places at once and a reader
//! does not know which one they are looking at (R7):
//!
//! * **frozen history** — logical lines, wrap-transparent, addressed by grapheme;
//! * **staged rows** — rows that have scrolled out of the grid but have not been finalized into a
//!   logical line yet, addressed by grapheme;
//! * **the live grid** — the screen itself, addressed by column.
//!
//! The last two are searched **row by row rather than line by line**, and that is a limit with a
//! reason rather than an omission: a logical line is a thing the transcript makes, and those two
//! planes have not made one yet. A wrapped command still being typed is two rows to everything in
//! this build — its anchors are two rows' worth, its cells are two rows' worth — and inventing a
//! join here would mean this module holding a private opinion about where lines are that the
//! anchors it paints through do not share. The row freezes, the transcript joins it, and the next
//! scan finds it as one line.
//!
//! # Where the numbers come from
//!
//! Every geometric constant below is a declaration of `.srchbar`'s stylesheet, named after it.
//! Two are *derived* rather than copied and say so: the capsule's top when the pane wears a head
//! (from `pane_head_geometry`, not from the mock-up's `38px`) and its right inset (from the
//! reserved scroll lane and the rail's own box, not from the mock-up's `28px`). Both derivations
//! exist so that moving the head's height or the lane's width moves the capsule with them, which
//! is the accident report at `cmdrail`'s own head read forwards.

use std::sync::Arc;

use bt_doc::{Bias, ContentAnchor, GridGeneration, GridPoint, ScreenId};
use bt_layout::SeatId;
use bt_render::{
    ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad, TERMINAL_SCROLL_LANE_LOGICAL_PX,
    rounded_overlay_fill,
};
use bt_transcript::search::{ByteRange, CompiledSearch, SearchError, SearchQuery, compile};
use bt_transcript::{GraphemeOffset, SourceGeneration, StagingId, TranscriptStore};
use bt_viewport::{SearchHighlights, SearchHit, SearchLine};

use crate::cmdrail::{RAIL_LANE_GAP_LOGICAL_PX, RAIL_PADDING_X_LOGICAL_PX, TICK_LENGTH_LOGICAL_PX};
use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::settings::push_float_window;
use crate::text_field::TextField;

// ── the capsule's own numbers (mock-up 1499-1537) ───────────────────────────

/// `.srchbar { top: 10px }` — how far below the pane's own top edge the capsule hangs when the
/// pane wears no head.
pub const CAPSULE_TOP_LOGICAL_PX: f32 = 10.0;
/// `.srchbar.with-head { top: 38px }`, **as the eight pixels it actually is**.
///
/// Thirty-eight is thirty plus eight, and thirty is `.panehead`'s height — so the mock-up's number
/// is a sum with one term that already exists as `SEAT_TITLE_BAR_LOGICAL_PX`. Writing the sum here
/// would mean a head that changed height leaving the capsule behind on top of it; taking the head's
/// own `content_bottom` and adding this is the same picture and cannot come apart.
pub const CAPSULE_HEAD_GAP_LOGICAL_PX: f32 = 8.0;
/// The gap between the command rail's box and the capsule's right edge.
///
/// `.srchbar { right: 28px }` against `.cmdrail { right: 11px }`: the rail's resting box is nine
/// pixels of tick with three of padding on each side, so it ends at 26, and the capsule starts two
/// further out. This is that two — the rest of the inset is derived from the rail's own constants
/// so that the two instruments keep their clearance when either moves.
pub const CAPSULE_RAIL_GAP_LOGICAL_PX: f32 = 2.0;
/// `.srchbar { border-radius: 8px }`.
pub const CAPSULE_RADIUS_LOGICAL_PX: f32 = 8.0;
/// `.srchbar { border: 1px solid var(--border) }`.
pub const CAPSULE_BORDER_LOGICAL_PX: f32 = 1.0;
/// `.srchbar { padding: 5px 6px }`, horizontal half.
pub const CAPSULE_PADDING_X_LOGICAL_PX: f32 = 6.0;
/// `.srchbar { padding: 5px 6px }`, vertical half.
pub const CAPSULE_PADDING_Y_LOGICAL_PX: f32 = 5.0;
/// `.srchbar { gap: 2px }` — the flex gap between its nine children.
pub const CAPSULE_GAP_LOGICAL_PX: f32 = 2.0;
/// `.srchbar input { width: 118px }`.
pub const FIELD_WIDTH_LOGICAL_PX: f32 = 118.0;
/// How narrow the field is allowed to get on a pane that cannot hold the capsule at its declared
/// width — about four monospace characters, which is the shortest box that still reads as one you
/// type into rather than as a gap between two buttons.
///
/// No stylesheet declares it: the mock-up's pane is always wide enough. It exists because a real
/// 300-pixel pane is not, and the alternative to a give is a control drawn in pieces.
pub const FIELD_MIN_WIDTH_LOGICAL_PX: f32 = 36.0;
/// `.srchbar input { padding: 4px }`.
pub const FIELD_PADDING_LOGICAL_PX: f32 = 4.0;
/// `.srchbar input { font: 12px/1.4 Consolas, monospace }` — **monospace, because what it takes is
/// terminal text** and a proportional face would make the query look unlike the thing it finds.
pub const FIELD_FONT_LOGICAL_PX: f32 = 12.0;
/// `.sb-cnt { min-width: 36px }`.
pub const COUNTER_MIN_WIDTH_LOGICAL_PX: f32 = 36.0;
/// `.sb-cnt { padding: 0 5px }`.
pub const COUNTER_PADDING_X_LOGICAL_PX: f32 = 5.0;
/// `.sb-cnt { font: 11px/1 Consolas, monospace }`.
pub const COUNTER_FONT_LOGICAL_PX: f32 = 11.0;
/// `.sb-tg { height: 22px }`.
pub const TOGGLE_HEIGHT_LOGICAL_PX: f32 = 22.0;
/// `.sb-tg { min-width: 21px }`.
pub const TOGGLE_MIN_WIDTH_LOGICAL_PX: f32 = 21.0;
/// `.sb-tg { padding: 0 3px }`.
pub const TOGGLE_PADDING_X_LOGICAL_PX: f32 = 3.0;
/// `.sb-tg { font: 11px/1 Consolas, monospace }`.
pub const TOGGLE_FONT_LOGICAL_PX: f32 = 11.0;
/// `.srchbar button { border-radius: 6px }`.
pub const BUTTON_RADIUS_LOGICAL_PX: f32 = 6.0;
/// `.sb-nav, .sb-x { width: 22px; height: 22px }`.
pub const BUTTON_BOX_LOGICAL_PX: f32 = 22.0;
/// The `10×10` chevron and cross inside those boxes.
pub const BUTTON_GLYPH_LOGICAL_PX: f32 = 10.0;
/// `.sb-sep { width: 1px }`.
pub const SEPARATOR_WIDTH_LOGICAL_PX: f32 = 1.0;
/// `.sb-sep { height: 16px }`.
pub const SEPARATOR_HEIGHT_LOGICAL_PX: f32 = 16.0;
/// `.sb-sep { margin: 0 3px }`.
pub const SEPARATOR_MARGIN_LOGICAL_PX: f32 = 3.0;
/// `.sb-tg.on { background: color-mix(in srgb, var(--accent) 14%, transparent) }`.
pub const TOGGLE_ON_GROUND_ALPHA: f32 = 0.14;
/// `.sb-tg[data-t="ww"] { text-decoration: underline; text-underline-offset: 2px }` — the `ab`
/// toggle's rule, drawn rather than declared (D-12/D-15).
///
/// There is no `text-decoration` in this window's label vocabulary, and there should not be one for
/// a single caller: what a text decoration *is*, once the glyphs are placed, is a rectangle under
/// them. So `ab` gets a one-pixel quad at this offset below its baseline box, which is VS Code's
/// own graphic for whole-word and the reason the two-letter label reads as anything at all.
pub const WORD_UNDERLINE_OFFSET_LOGICAL_PX: f32 = 2.0;
/// The caret's width in the field — the window's own one-pixel bar, as the graph's field draws it.
pub const FIELD_CARET_LOGICAL_PX: f32 = 1.0;
/// How far the caret is held off the field box's own top and bottom.
pub const FIELD_CARET_INSET_LOGICAL_PX: f32 = 4.0;

/// Where a hit that had to be scrolled to lands: **one third of the way down the viewport**
/// (mock 8596, `offsetTop - clientHeight/3`).
///
/// Not the top, which is where the rail's own jump puts a command. A command mark is the *start* of
/// something you are about to read downwards, so it belongs at the top with its output below it; a
/// search hit is a point in the middle of text you want to read *around*, so it is given a third of
/// a screen of context above it.
pub const HIT_LANDING_FRACTION: f32 = 1.0 / 3.0;

/// `placeholder="Find"` (mock 8683) — **`Find`, not `Search`**, which is the verb every editor on
/// this platform prints in this box.
#[must_use]
pub fn field_placeholder() -> &'static str {
    crate::i18n::Text::SearchPlaceholder.text()
}
/// `.sb-tg[data-t="cs"]` — `Aa`.
pub const CASE_LABEL: &str = "Aa";
/// `.sb-tg[data-t="ww"]` — `ab`, underlined.
pub const WORD_LABEL: &str = "ab";
/// `.sb-tg[data-t="re"]` — `.*`.
pub const REGEX_LABEL: &str = ".*";
/// The counter when the pattern will not compile (B51).
pub const BROKEN_COUNT: &str = "\u{2014}";
/// The counter when the pattern compiled and found nothing (B51).
///
/// **`0/0` and not "No results"**, which the prototype spells out as a literal beside the other
/// three. It is the same shape as the answer next to it — `3/17` — so the eye reads the pair as one
/// number that went to zero rather than as the box changing what kind of thing it says.
pub const EMPTY_COUNT: &str = "0/0";

/// `title="Match case"`.
#[must_use]
pub fn case_tip() -> &'static str {
    crate::i18n::Text::SearchTipCase.text()
}
/// `title="Whole word"`.
#[must_use]
pub fn word_tip() -> &'static str {
    crate::i18n::Text::SearchTipWord.text()
}
/// `title="Regular expression"`.
#[must_use]
pub fn regex_tip() -> &'static str {
    crate::i18n::Text::SearchTipRegex.text()
}
/// `title="Previous match (Shift+Enter)"`.
#[must_use]
pub fn previous_tip() -> &'static str {
    crate::i18n::Text::SearchTipPrevious.text()
}
/// `title="Next match (Enter)"`.
#[must_use]
pub fn next_tip() -> &'static str {
    crate::i18n::Text::SearchTipNext.text()
}
/// `title="Close (Esc)"`.
#[must_use]
pub fn close_tip() -> &'static str {
    crate::i18n::Text::SearchTipClose.text()
}

// ── the state ───────────────────────────────────────────────────────────────

/// The three toggles, all off (B43, D-4).
///
/// **Remembered for the process and never written to disk** (D-4). They live beside the query in
/// one value that `close` does not touch, so re-opening comes back exactly as you left it; and
/// nothing writes them into `bt-persist`, because a search is a thing you are doing and not a thing
/// this window is. A restored session that came back case-sensitive because of something you did on
/// Tuesday would be a preference nobody set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchFlags {
    /// `Aa` — off means the fold is on, which is the default every find bar has.
    pub case_sensitive: bool,
    /// `ab` — `\b…\b`, ASCII word breaks (D-6; the engine's own note says why CJK cannot).
    pub whole_word: bool,
    /// `.*` — the typed text is a pattern rather than a literal.
    pub regex: bool,
}

/// Which toggle a press means.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchFlag {
    Case,
    Word,
    Regex,
}

impl SearchFlags {
    /// Flip one, and say so.
    pub fn toggle(&mut self, flag: SearchFlag) {
        let bit = match flag {
            SearchFlag::Case => &mut self.case_sensitive,
            SearchFlag::Word => &mut self.whole_word,
            SearchFlag::Regex => &mut self.regex,
        };
        *bit = !*bit;
    }

    #[must_use]
    pub fn is_on(self, flag: SearchFlag) -> bool {
        match flag {
            SearchFlag::Case => self.case_sensitive,
            SearchFlag::Word => self.whole_word,
            SearchFlag::Regex => self.regex,
        }
    }

    /// The engine's own question, built from what is typed and what is switched on.
    #[must_use]
    pub fn query(self, text: &str) -> SearchQuery {
        SearchQuery {
            text: text.to_owned(),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            regex: self.regex,
        }
    }
}

/// One hit, at the two addresses the window needs it at.
///
/// [`Self::line`] and the offsets are what the highlighter paints through; [`Self::anchor`] is what
/// the jump scrolls to. They are the same place said twice because they are consumed by two layers
/// that do not share a vocabulary — the projection knows planes and offsets, the scroll knows
/// anchors — and deriving one from the other at the point of use would mean the transcript being
/// asked again for a line that may by then have been evicted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hit {
    pub line: SearchLine,
    /// Half-open, in the line's own unit: graphemes on the two transcript planes, columns on the
    /// live grid.
    pub start: u32,
    pub end: u32,
    /// Where the hit begins, as the document coordinate a viewport can be anchored to.
    pub anchor: ContentAnchor,
}

impl Hit {
    /// The address the projection paints from.
    #[must_use]
    pub fn highlight(&self) -> SearchHit {
        SearchHit {
            line: self.line,
            start: self.start,
            end: self.end,
        }
    }

    /// Whether this is the same hit as `other` **by identity rather than by index** — the same
    /// range of the same line.
    ///
    /// This is what carries the current match across a rebuild (R2). Output arriving while the
    /// capsule is open re-scans everything, and the hit the reader is standing on has to survive
    /// that: its index has almost certainly moved, its line has not.
    #[must_use]
    pub fn is(&self, other: &Self) -> bool {
        self.line == other.line && self.start == other.start && self.end == other.end
    }
}

/// What the counter says (B51) — four states, and the prototype's own literals for all four.
#[must_use]
pub fn counter_text(
    query_empty: bool,
    broken: bool,
    hits: usize,
    current: Option<usize>,
) -> String {
    if query_empty {
        return String::new();
    }
    if broken {
        return BROKEN_COUNT.to_owned();
    }
    match current {
        Some(at) if hits > 0 => format!("{}/{hits}", at + 1),
        _ => EMPTY_COUNT.to_owned(),
    }
}

/// The whole of one window's search (mock 8515's `srch`, one object).
#[derive(Clone, Debug, Default)]
pub struct SearchState {
    /// Which pane the capsule is on, or `None` when it is not up.
    ///
    /// **The one field that says whether there is a capsule at all.** The query, the flags and the
    /// caret are all kept through a close (B62: *"`srch.q` is not cleared — the query stays"*), so
    /// "is the search open" cannot be read off any of them.
    seat: Option<SeatId>,
    /// What is typed, and the caret in it.
    field: TextField,
    flags: SearchFlags,
    /// Whether the field holds the keyboard.
    ///
    /// **Not the same question as "is the capsule open"** (B81). The prototype's `F3` walks the
    /// matches *while the terminal has the keyboard*, which is the second of the two stances a
    /// reader can be in: search open, hands back on the shell. So the capsule being up and the
    /// caret being in it are two facts, and this is the second one.
    focused: bool,
    /// Every hit, in document order.
    hits: Vec<Hit>,
    /// Which of them is current, as an index into [`Self::hits`].
    current: Option<usize>,
    /// What the regex engine said, when it refused the pattern. `Some` is exactly the `.bad` state
    /// (A28: the typed text turns red, said where it is typed) and the message is the tip.
    error: Option<String>,
    /// The hit set in the shape a projection paints from, rebuilt with the hits and shared.
    highlights: Arc<SearchHighlights>,
    /// Bumped wherever [`Self::hits`] is replaced — [`Self::install`], and the two doors that empty
    /// it — and by nothing else.
    ///
    /// **The rail's half of the cache key** (S4). The results rail is a picture of the hit set, so
    /// it has to be rebuilt exactly when that set is replaced and never per frame — the same
    /// bargain `command_marks_revision` strikes for the ledger, said in the same word so that
    /// `RailKey` can hold the two side by side. A counter rather than a hash of the hits because
    /// `install` is the one door they come through: anything that changed them went through it,
    /// and anything that did not, did not.
    revision: u64,
    /// **The tally of a host that counts its own matches** (§7.7 ②, W2 slice ④).
    ///
    /// The capsule's second host is a page, and a page's matches are not
    /// [`Hit`]s: a `Hit` is a range of a transcript line addressed by a
    /// [`ContentAnchor`], and a document inside an engine has neither. What the
    /// engine reports is a count and which match is current, so that is what is
    /// kept — `(count, active)` with `active` 1-based and `0` for none, exactly
    /// as `ICoreWebView2Find` states them.
    ///
    /// `None` means "this host counts through [`Self::hits`]", which is what
    /// keeps a terminal's capsule byte-identical to what it was.
    engine_matches: Option<(i32, i32)>,
    /// Whether the host under this capsule counts its own matches at all.
    ///
    /// Separate from [`Self::engine_matches`] because the two say different
    /// things and the difference is a number on the glass: a page that has not
    /// been asked yet has **no** count, and `0/0` on it would be the capsule
    /// claiming an answer nobody has given.
    counts_its_own: bool,
}

impl SearchState {
    #[must_use]
    pub fn seat(&self) -> Option<SeatId> {
        self.seat
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.seat.is_some()
    }

    #[must_use]
    pub fn is_focused(&self) -> bool {
        self.seat.is_some() && self.focused
    }

    #[must_use]
    pub fn field(&self) -> &TextField {
        &self.field
    }

    pub fn field_mut(&mut self) -> &mut TextField {
        &mut self.field
    }

    #[must_use]
    pub fn query(&self) -> &str {
        self.field.text()
    }

    #[must_use]
    pub fn flags(&self) -> SearchFlags {
        self.flags
    }

    pub fn flags_mut(&mut self) -> &mut SearchFlags {
        &mut self.flags
    }

    #[must_use]
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    #[must_use]
    pub fn current(&self) -> Option<&Hit> {
        self.hits.get(self.current?)
    }

    /// Which hit is current, as an index — the number the results rail is keyed on.
    ///
    /// The hit itself is what the highlighter and the scroll want; the *index* is what a cache key
    /// wants, because two hits can be equal by value at different places in the walk and the rail's
    /// `.cur` tick has to move when the walk does.
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// How many times the hit set has been replaced. See [`Self::revision`]'s field.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[must_use]
    pub fn highlights(&self) -> &Arc<SearchHighlights> {
        &self.highlights
    }

    /// What the counter box reads right now.
    ///
    /// **One counter, two ways of arriving at the numbers** (§7.7 ②: 「换的是谁
    /// 数命中,不换胶囊」). A terminal's are counted here out of [`Self::hits`];
    /// a page's are counted by the engine and reported. `counter_text` is the
    /// same function either way, so the four states it writes — empty, broken,
    /// `n/m`, `0/0` — are the same four wherever the capsule is standing.
    #[must_use]
    pub fn counter(&self) -> String {
        if self.counts_its_own && self.engine_matches.is_none() {
            // Asked for and not yet answered. Empty is what the capsule already
            // says for a query nobody has typed, and it means the same thing
            // here: there is no count, as against a count of none.
            return String::new();
        }
        if let Some((count, active)) = self.engine_matches {
            return counter_text(
                self.field.is_empty(),
                self.error.is_some(),
                count.max(0) as usize,
                (active > 0).then(|| active as usize - 1),
            );
        }
        counter_text(
            self.field.is_empty(),
            self.error.is_some(),
            self.hits.len(),
            self.current,
        )
    }

    /// A host that counts its own matches has reported.
    ///
    /// Returns whether the tally moved, so the caller can leave the chrome alone
    /// on the several events a single find sends for one answer.
    pub fn report_engine_matches(&mut self, count: i32, active: i32) -> bool {
        let reported = Some((count, active));
        let moved = self.engine_matches != reported;
        self.engine_matches = reported;
        moved
    }

    /// This host counts through [`Self::hits`] again — or has not answered yet.
    ///
    /// Called on every door that changes what is being searched, so a stale
    /// `12/40` cannot survive the query that produced it.
    pub fn forget_engine_matches(&mut self) {
        self.engine_matches = None;
    }

    /// Say whether the host under this capsule counts its own matches.
    pub fn set_counts_its_own(&mut self, counts: bool) {
        self.counts_its_own = counts;
    }

    /// Open on a pane, or re-focus the one already open there.
    ///
    /// **Re-opening keeps the query and selects it** (B76): *"the last query comes back selected —
    /// type to replace, Enter to reuse"*. Opening on a *different* pane closes the first, because
    /// there is one search.
    ///
    /// Returns whether anything about the search's shape changed, so a caller knows whether a
    /// rescan is owed.
    pub fn open(&mut self, seat: SeatId) -> bool {
        let moved = self.seat != Some(seat);
        if moved {
            self.hits.clear();
            self.current = None;
            self.error = None;
            self.highlights = Arc::default();
            self.revision = self.revision.wrapping_add(1);
        }
        self.seat = Some(seat);
        self.focused = true;
        self.field.select_all();
        self.engine_matches = None;
        moved
    }

    /// Put the capsule away, keeping what was typed and how it was switched.
    ///
    /// **Nothing scrolls** (B63). The viewport stays where the walk left it, which is what makes
    /// the capsule a way of *travelling* through the scrollback rather than a modal you look
    /// through and then leave.
    ///
    /// Returns whether it was open, so a caller can tell "Esc closed the search" from "Esc has
    /// nothing here to close and belongs to the layer below".
    pub fn close(&mut self) -> bool {
        let was_open = self.seat.take().is_some();
        self.focused = false;
        self.engine_matches = None;
        self.hits.clear();
        self.current = None;
        self.error = None;
        self.highlights = Arc::default();
        self.revision = self.revision.wrapping_add(1);
        was_open
    }

    /// Give the keyboard back to the pane without putting the capsule away (B81's second stance).
    pub fn blur(&mut self) {
        self.focused = false;
    }

    /// Take the keyboard back — a press anywhere on the capsule does this (B74: *"the capsule is
    /// one control: any press hands the caret back"*).
    pub fn focus(&mut self) {
        self.focused = true;
    }

    /// Install a fresh scan.
    ///
    /// `keep_current` is B58's rule, and the two halves of that rule are the whole of why this
    /// takes an argument at all:
    ///
    /// * **A rebuild that was not asked for keeps the reader where they are.** Output arrived, or a
    ///   line was evicted; the hit under the eye is found again by identity and stays current.
    /// * **A rebuild the reader caused starts from where the eye is** — *"the first match at or
    ///   below the viewport top"*, which is `from` here. Typing another letter must not throw the
    ///   view to the top of the scrollback.
    ///
    /// `from` is the document position the viewport is showing, when there is one; `None` means the
    /// pane is at the live bottom, where "at or below the top" is every hit and the answer is the
    /// first.
    pub fn install(
        &mut self,
        hits: Vec<Hit>,
        error: Option<String>,
        keep_current: bool,
        from: Option<&ContentAnchor>,
    ) {
        let standing = keep_current.then(|| self.current()).flatten().cloned();
        self.revision = self.revision.wrapping_add(1);
        self.hits = hits;
        self.error = error;
        self.current = if self.hits.is_empty() {
            None
        } else if let Some(standing) = standing {
            // The same hit if it survived; otherwise **the nearest one after it**, which is where
            // the reader was heading. A line evicted from under the caret moves the walk forwards
            // rather than throwing it back to the beginning of the scrollback.
            Some(
                self.hits
                    .iter()
                    .position(|hit| hit.is(&standing))
                    .or_else(|| {
                        self.hits.iter().position(|hit| {
                            (hit.line, hit.start) >= (standing.line, standing.start)
                        })
                    })
                    .unwrap_or(self.hits.len() - 1),
            )
        } else {
            Some(from.map_or(0, |anchor| self.first_at_or_after(anchor)))
        };
        self.rebuild_highlights();
    }

    /// The first hit standing at or after `anchor`, or the first of all when none does.
    fn first_at_or_after(&self, anchor: &ContentAnchor) -> usize {
        self.hits
            .iter()
            .position(|hit| {
                bt_doc::compare_anchors(&hit.anchor, anchor).is_ok_and(std::cmp::Ordering::is_ge)
            })
            .unwrap_or(0)
    }

    /// Walk one hit forwards or backwards, **with wrap-around** (B61).
    ///
    /// A ring, unlike the command rail's walk, and the two are opposites on purpose: a rail is a
    /// history with a beginning and an end, while a search is a set of places in a text and
    /// stepping off the last one means "start again".
    ///
    /// Returns the hit stepped to, when there is one.
    pub fn step(&mut self, forwards: bool) -> Option<&Hit> {
        let count = self.hits.len();
        if count == 0 {
            return None;
        }
        let at = self.current.unwrap_or(if forwards { count - 1 } else { 0 });
        let next = if forwards {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        self.current = Some(next);
        self.rebuild_highlights();
        self.hits.get(next)
    }

    /// Make one hit current outright.
    ///
    /// **The hook S4 lands on**: when the command rail doubles as the results rail, a press on a
    /// match tick means "this one", which is neither a step forwards nor a step back. It is here
    /// rather than in that slice because it is the *state's* verb and the state is finished — the
    /// rail will bring a tick index and nothing else.
    pub fn set_current(&mut self, index: usize) -> Option<&Hit> {
        if index >= self.hits.len() {
            return None;
        }
        self.current = Some(index);
        self.rebuild_highlights();
        self.hits.get(index)
    }

    fn rebuild_highlights(&mut self) {
        let current = self
            .current
            .and_then(|at| self.hits.get(at))
            .map(Hit::highlight);
        self.highlights = Arc::new(SearchHighlights::new(
            self.hits.iter().map(Hit::highlight),
            current,
        ));
    }
}

// ── the scan ────────────────────────────────────────────────────────────────

/// One row of the live grid, as the searcher wants it: its text, and the column each grapheme of
/// that text starts at.
///
/// The columns are carried rather than recomputed because the row's cells already say them — a wide
/// glyph occupies two and its spacer is not a character — and re-deriving them from the joined
/// string would mean measuring widths a second time and getting a second answer.
pub struct LiveRow {
    /// The grid row this is.
    pub row: u32,
    /// The row's text, cell by cell.
    pub text: String,
    /// `columns[k]` is the byte at which the cluster occupying column `k` starts; the trailing
    /// entry is the text's length, so a byte range converts to a column range by two searches.
    pub column_starts: Vec<u32>,
}

/// Turn one live grid row into a searchable line.
#[must_use]
pub fn live_row(row: u32, cells: &[bt_transcript::CapturedCell]) -> LiveRow {
    let mut text = String::new();
    let mut column_starts = Vec::with_capacity(cells.len() + 1);
    for cell in cells {
        column_starts.push(text.len() as u32);
        // A wide glyph's spacer column carries no text of its own; it shares the byte the lead
        // cell starts at, which is what makes a hit over a CJK character cover both its columns.
        if !cell.wide_spacer {
            text.push_str(&cell.text);
        }
    }
    column_starts.push(text.len() as u32);
    LiveRow {
        row,
        text,
        column_starts,
    }
}

/// Where a byte lands in a line whose units start at `boundaries`.
///
/// `boundaries` is ascending and ends with the text's length — a grapheme boundary table or a
/// column start table, which are the same shape used for the same purpose. A byte inside a unit
/// belongs to that unit, which is what makes a hit that begins mid-cluster cover the whole cluster
/// rather than half of one.
fn unit_at(boundaries: &[u32], byte: u32) -> u32 {
    boundaries
        .partition_point(|start| *start <= byte)
        .saturating_sub(1) as u32
}

/// The half-open unit range a byte range covers.
fn unit_range(boundaries: &[u32], range: ByteRange) -> (u32, u32) {
    let start = unit_at(boundaries, range.start as u32);
    // The unit holding the last byte *inside* the range, plus one: a range that ends on a
    // boundary must not claim the unit that begins there.
    let last = unit_at(boundaries, (range.end as u32).saturating_sub(1));
    (start, last.saturating_add(1).max(start.saturating_add(1)))
}

/// The frozen plane, scanned.
///
/// **Split from [`scan_volatile`] because the two are re-asked at different rates**, which is the
/// whole of this block's answer to R1: history is append-and-evict-only and costs about twelve
/// milliseconds at a hundred thousand lines, so it is scanned when it moves; the other two planes
/// are fifty rows between them and are scanned every time anything is asked. The split is honest
/// rather than a shortcut, because the frozen plane genuinely cannot change under a caller that has
/// not seen its length, its two end ids or its generation move.
#[must_use]
pub fn scan_history(compiled: &CompiledSearch, transcript: &TranscriptStore) -> Vec<Hit> {
    let mut hits = Vec::new();
    for line in transcript.frozen() {
        push_hits(
            &mut hits,
            compiled,
            &line.text,
            &line.grapheme_boundaries,
            SearchLine::History(line.id),
            |offset| ContentAnchor::History {
                id: line.id,
                offset: GraphemeOffset(offset),
                bias: Bias::Before,
                generation: line.source_generation,
            },
        );
    }
    hits
}

/// The staged rows and the live grid, scanned.
///
/// **Staging before the live grid**, which is `Staging < Live` from §3.2 — the document's own total
/// order, and the order the caller then appends this list to the history one in. Every walk, every
/// count and every "first hit at or below here" downstream depends on the whole list being in
/// document order without anybody re-sorting it.
///
/// Both planes are searched **row by row**. See this module's own head for why that is a limit with
/// a reason rather than an omission: neither plane has a logical line yet, and inventing one here
/// would mean this module holding a private opinion about where lines are that the anchors it
/// paints through do not share.
#[must_use]
pub fn scan_volatile(
    compiled: &CompiledSearch,
    transcript: &TranscriptStore,
    live: &[LiveRow],
    grid_generation: GridGeneration,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    let staging_generation = transcript.source_generation();
    for staged in transcript.staged_rows() {
        let row = live_row(0, &staged.row.cells);
        push_hits(
            &mut hits,
            compiled,
            &row.text,
            &row.column_starts,
            SearchLine::Staging(staged.id),
            |offset| staging_anchor(staged.id, offset, staging_generation),
        );
    }
    for row in live {
        push_hits(
            &mut hits,
            compiled,
            &row.text,
            &row.column_starts,
            SearchLine::Live { row: row.row },
            |offset| ContentAnchor::Live {
                screen: ScreenId::Primary,
                point: GridPoint {
                    row: row.row,
                    column: offset,
                },
                bias: Bias::Before,
                generation: grid_generation,
            },
        );
    }
    hits
}

fn staging_anchor(id: StagingId, offset: u32, generation: SourceGeneration) -> ContentAnchor {
    ContentAnchor::Staging {
        id,
        offset: GraphemeOffset(offset),
        bias: Bias::Before,
        generation,
    }
}

fn push_hits(
    into: &mut Vec<Hit>,
    compiled: &CompiledSearch,
    text: &str,
    boundaries: &[u32],
    line: SearchLine,
    anchor: impl Fn(u32) -> ContentAnchor,
) {
    for range in bt_transcript::search::find_in_line(compiled, text) {
        let (start, end) = unit_range(boundaries, range);
        into.push(Hit {
            line,
            start,
            end,
            anchor: anchor(start),
        });
    }
}

/// Which toggle `Alt+<letter>` means, if any.
///
/// **VS Code's three**, because the prototype gives the toggles a click and nothing else (B74) —
/// there is no rule of the mock-up's to follow here, so the reference product for this surface
/// supplies one rather than this file inventing a fourth convention.
///
/// A free function for [`crate::graph_key_of`]'s reason: what `Alt+W` *does* is a property of the
/// capsule and has to be assertable without a keyboard, while the translation from winit lives
/// beside the window that reads winit.
///
/// Case-folded, because `Alt+Shift+W` is still the reader reaching for whole-word — every other
/// letter binding in this window folds the same way.
#[must_use]
pub fn toggle_for_letter(letter: &str) -> Option<SearchFlag> {
    match letter.to_lowercase().as_str() {
        "c" => Some(SearchFlag::Case),
        "w" => Some(SearchFlag::Word),
        "r" => Some(SearchFlag::Regex),
        _ => None,
    }
}

/// Compile what is typed, or say why it will not.
///
/// Three answers and not two, which is [`SearchError`]'s own division: nothing typed is a state the
/// capsule sits in and shows no count for, a broken pattern is a red field with a message, and
/// anything else is an engine.
pub fn engine(flags: SearchFlags, text: &str) -> Result<CompiledSearch, SearchError> {
    compile(&flags.query(text))
}

// ── the capsule's geometry ──────────────────────────────────────────────────

/// Which of the capsule's parts a point is on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchElement {
    /// The text field — a press puts the caret in it.
    Field,
    Toggle(SearchFlag),
    Previous,
    Next,
    Close,
    /// The capsule's own body, between its controls. A press here is still the capsule's — B74's
    /// *"any press hands the caret back"* — and must not fall through to the pane beneath.
    Body,
}

/// Where every part of one capsule stands, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    /// The border box.
    pub frame: [f32; 4],
    /// The text field's box, padding included.
    pub field: [f32; 4],
    /// The counter's box, right-aligned inside it.
    pub counter: [f32; 4],
    pub case: [f32; 4],
    pub word: [f32; 4],
    pub regex: [f32; 4],
    /// The hairline between the toggles and the walk.
    pub separator: [f32; 4],
    pub previous: [f32; 4],
    pub next: [f32; 4],
    pub close: [f32; 4],
}

impl Capsule {
    /// **The caret's line box** — its x from the measured text in front of it, its
    /// top and bottom the field's own, in physical window pixels.
    ///
    /// One derivation with two readers, which is [`hit`]'s own discipline moved to
    /// the caret: [`build`] insets this to draw the bar, and the window hands the
    /// very same rectangle to the IME so the candidate list stands under the box
    /// the letters are going into. A candidate window placed from a second
    /// computation is a candidate window that drifts off the caret it claims to
    /// follow — and, before this existed, a search field that published no
    /// rectangle at all left the list parked in the window's corner (user report,
    /// 2026-08-17).
    ///
    /// The **line box** and not the bar, because the two are asked different
    /// questions: the bar is the ink an insertion point is drawn in, held four
    /// pixels off the field's edges so it does not touch them, while the IME is
    /// being told which line it must not cover — and a rectangle four pixels short
    /// is four pixels of the field the candidate list is allowed to sit on.
    #[must_use]
    pub fn caret_line(&self, caret_x: f32, scale: f32) -> [f32; 4] {
        let px = |logical: f32| logical * scale;
        let caret = px(FIELD_CARET_LOGICAL_PX).round().max(1.0);
        let text_left = self.field[0] + px(FIELD_PADDING_LOGICAL_PX);
        let text_right = (self.field[2] - px(FIELD_PADDING_LOGICAL_PX)).max(text_left);
        let x = (text_left + caret_x).min(text_right - caret);
        [x, self.field[1], x + caret, self.field[3]]
    }

    /// One toggle's box.
    #[must_use]
    pub fn toggle(&self, flag: SearchFlag) -> [f32; 4] {
        match flag {
            SearchFlag::Case => self.case,
            SearchFlag::Word => self.word,
            SearchFlag::Regex => self.regex,
        }
    }
}

/// What a point on the capsule means, or `None` when the point is not on it.
#[must_use]
pub fn hit(capsule: &Capsule, x: f32, y: f32) -> Option<SearchElement> {
    let inside = |rect: [f32; 4]| x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3];
    if !inside(capsule.frame) {
        return None;
    }
    Some(if inside(capsule.field) {
        SearchElement::Field
    } else if inside(capsule.case) {
        SearchElement::Toggle(SearchFlag::Case)
    } else if inside(capsule.word) {
        SearchElement::Toggle(SearchFlag::Word)
    } else if inside(capsule.regex) {
        SearchElement::Toggle(SearchFlag::Regex)
    } else if inside(capsule.previous) {
        SearchElement::Previous
    } else if inside(capsule.next) {
        SearchElement::Next
    } else if inside(capsule.close) {
        SearchElement::Close
    } else {
        SearchElement::Body
    })
}

/// How far in from a pane's right edge the capsule's own right edge stands.
///
/// Derived rather than the mock-up's `28px`: the reserved scroll lane, the rail's gap off it, the
/// rail's resting box, and this module's own two pixels of air. The stylesheet's comment beside the
/// rail is an accident report — *"the rail and the thumb are different instruments and may not
/// share a lane"* — and the capsule is a third instrument in the same corner. One number moves all
/// three.
#[must_use]
pub fn right_inset_logical_px() -> f32 {
    TERMINAL_SCROLL_LANE_LOGICAL_PX
        + RAIL_LANE_GAP_LOGICAL_PX
        + 2.0 * RAIL_PADDING_X_LOGICAL_PX
        + TICK_LENGTH_LOGICAL_PX
        + CAPSULE_RAIL_GAP_LOGICAL_PX
}

/// Lay one capsule out inside a pane.
///
/// `seat` is the pane's whole rectangle — **not its body** — because the mock-up's origin is
/// `.pane-inner`, which includes the head. `head_content_bottom` is `pane_head_geometry`'s own
/// answer when the pane wears a head, which is what `.with-head` derives its top from (A26).
///
/// `field_width` and `counter_width` are measured text: the field keeps its declared 118 pixels
/// unless what is typed is wider, and the counter keeps its declared 36 unless the number is.
/// Neither shrinks, so the capsule never jitters as a count goes from `9/12` to `10/12`.
#[must_use]
pub fn lay_out(
    seat: [f32; 4],
    head_content_bottom: Option<f32>,
    scale: f32,
    counter_width: f32,
) -> Capsule {
    let px = |logical: f32| logical * scale;
    let pad_x = px(CAPSULE_PADDING_X_LOGICAL_PX);
    let pad_y = px(CAPSULE_PADDING_Y_LOGICAL_PX);
    let gap = px(CAPSULE_GAP_LOGICAL_PX);
    let button = px(BUTTON_BOX_LOGICAL_PX).round();
    let toggle_height = px(TOGGLE_HEIGHT_LOGICAL_PX).round();
    let toggle_width = px(TOGGLE_MIN_WIDTH_LOGICAL_PX).round();
    let field_height = px(FIELD_FONT_LOGICAL_PX * 1.4 + 2.0 * FIELD_PADDING_LOGICAL_PX).round();
    let counter_width = counter_width.max(px(COUNTER_MIN_WIDTH_LOGICAL_PX));
    let separator_margin = px(SEPARATOR_MARGIN_LOGICAL_PX);
    let separator_width = px(SEPARATOR_WIDTH_LOGICAL_PX).max(1.0).round();

    // The tallest child decides the row's height, which is what `align-items: center` means.
    let content_height = field_height.max(toggle_height).max(button);
    // Everything except the field, which is the only child with anything to give.
    let fixed = pad_x * 2.0
        + gap
        + counter_width
        + gap
        + toggle_width * 3.0
        + gap * 2.0
        + gap
        + separator_margin * 2.0
        + separator_width
        + gap
        + button * 3.0
        + gap * 2.0;
    let right = (seat[2] - px(right_inset_logical_px())).round();
    // **The field is the child that gives** — the pane head's own rule (`.ptitle` is the flex
    // child that shrinks so the controls keep their boxes), applied to the one child here that has
    // a natural give: a query box can be short and still be a query box, while a `×` at 18 pixels
    // is a `×` you cannot press.
    //
    // The first cut had no give at all and clamped the *frame's* left edge instead, which shortens
    // the box without shortening the row inside it: a real 300-pixel pane drew the capsule with
    // its own cross outside its own rounded corner (photographed 2026-08-16). Shrinking the field
    // is what keeps the whole control on a pane that narrow, and below the floor the capsule
    // simply hangs off the left edge the way an absolutely positioned box in a `position:
    // relative` parent does — legible, and never in pieces.
    let field_width = px(FIELD_WIDTH_LOGICAL_PX)
        .min((right - seat[0] - fixed).max(px(FIELD_MIN_WIDTH_LOGICAL_PX)))
        .round();
    let width = fixed + field_width;
    let height = content_height + pad_y * 2.0;
    let left = (right - width).round();
    let top = head_content_bottom
        .map_or(seat[1] + px(CAPSULE_TOP_LOGICAL_PX), |bottom| {
            bottom + px(CAPSULE_HEAD_GAP_LOGICAL_PX)
        })
        .round();
    let frame = [left, top, right, top + height];

    // The row, laid left to right; each child is centred vertically in the content band.
    let centre = |box_height: f32, x: f32, box_width: f32| {
        let y = (top + pad_y + (content_height - box_height) / 2.0).round();
        [x, y, x + box_width, y + box_height]
    };
    let mut x = left + pad_x;
    let field = centre(field_height, x, field_width);
    x = field[2] + gap;
    let counter = centre(content_height, x, counter_width);
    x = counter[2] + gap;
    let case = centre(toggle_height, x, toggle_width);
    x = case[2] + gap;
    let word = centre(toggle_height, x, toggle_width);
    x = word[2] + gap;
    let regex = centre(toggle_height, x, toggle_width);
    x = regex[2] + gap + separator_margin;
    let separator = centre(px(SEPARATOR_HEIGHT_LOGICAL_PX).round(), x, separator_width);
    x = separator[2] + separator_margin + gap;
    let previous = centre(button, x, button);
    x = previous[2] + gap;
    let next = centre(button, x, button);
    x = next[2] + gap;
    let close = centre(button, x, button);

    Capsule {
        frame,
        field,
        counter,
        case,
        word,
        regex,
        separator,
        previous,
        next,
        close,
    }
}

// ── the picture ─────────────────────────────────────────────────────────────

/// What the capsule is showing this frame.
pub struct CapsuleLook<'a> {
    /// What to print in the field: the typed text with the composition spliced in at the caret, or
    /// the placeholder when there is neither.
    pub text: &'a str,
    /// Whether [`Self::text`] is the reader's or the placeholder — the placeholder is the field
    /// saying what it is *for*, and it may not wear the ink of a query somebody typed.
    pub typed: bool,
    /// How wide the text in front of the caret is, for the caret's own x.
    pub caret_x: f32,
    /// Whether the field holds the keyboard, which is whether a caret is drawn at all.
    pub focused: bool,
    /// The regex did not parse: the typed text turns red **where it is typed** (A28).
    pub broken: bool,
    /// What the counter box reads.
    pub counter: &'a str,
    pub flags: SearchFlags,
    /// **Which toggles the host under the capsule can actually honour**
    /// (§7.7 ②, W2 slice ④).
    ///
    /// A terminal answers all three. A page answers `Aa` and no more:
    /// `ICoreWebView2FindOptions` carries a find term, a case fold and a
    /// highlight-all, and there is no word-boundary and no pattern anywhere in
    /// that interface.
    ///
    /// **Dimmed and inert rather than gone**, which is this slice's own ruling
    /// about the navigation buttons applied to the same problem one surface
    /// over: 「a button that vanishes when the history runs out moves the two
    /// beside it under the pointer」. A capsule that grew and shrank between
    /// hosts would be a second capsule.
    pub offered: SearchFlags,
    /// Which element the pointer is on, if any.
    pub hover: Option<SearchElement>,
}

/// Draw one capsule.
#[must_use]
pub fn build(
    capsule: &Capsule,
    look: &CapsuleLook<'_>,
    palette: &ChromePalette,
    scale: f32,
) -> OverlayLayer {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads: Vec<OverlayQuad> = Vec::new();
    let mut labels: Vec<ChromeLabel> = Vec::new();
    let mut sprites: Vec<ChromeSprite> = Vec::new();

    push_float_window(
        &mut quads,
        capsule.frame,
        px(CAPSULE_RADIUS_LOGICAL_PX),
        px(CAPSULE_BORDER_LOGICAL_PX),
        px(bt_render::FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.menu_shadow_inner_alpha),
        alpha(palette.menu_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    // The field's text, at the field's own left padding.
    let text_left = capsule.field[0] + px(FIELD_PADDING_LOGICAL_PX);
    let text_rect = [
        text_left,
        capsule.field[1],
        capsule.field[2] - px(FIELD_PADDING_LOGICAL_PX),
        capsule.field[3],
    ];
    labels.push(ChromeLabel {
        mono: false,
        text: look.text.to_owned(),
        rect: text_rect,
        font_size_px: px(FIELD_FONT_LOGICAL_PX),
        color: match (look.broken, look.typed) {
            // `.srchbar input.bad { color: var(--err) }` — the pattern is broken and the field
            // says so in place, with no card and no banner.
            (true, _) => palette.status_err,
            (false, true) => palette.menu_item_text_selected,
            (false, false) => palette.menu_item_text,
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(text_rect),
    });
    if look.focused {
        let inset = px(FIELD_CARET_INSET_LOGICAL_PX).round();
        let line = capsule.caret_line(look.caret_x, scale);
        quads.push(OverlayQuad {
            rect: [line[0], line[1] + inset, line[2], line[3] - inset],
            color: palette.accent,
            alpha: 1.0,
        });
    }

    if !look.counter.is_empty() {
        let rect = [
            capsule.counter[0],
            capsule.counter[1],
            capsule.counter[2] - px(COUNTER_PADDING_X_LOGICAL_PX),
            capsule.counter[3],
        ];
        labels.push(ChromeLabel {
            mono: false,
            text: look.counter.to_owned(),
            rect,
            font_size_px: px(COUNTER_FONT_LOGICAL_PX),
            color: palette.menu_item_hint_text,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            // `3/17` is a pair of numbers that changes every keystroke; proportional digits would
            // make the box breathe under a walk.
            tabular_numerals: true,
            clip: Some(rect),
        });
    }

    for (flag, text) in [
        (SearchFlag::Case, CASE_LABEL),
        (SearchFlag::Word, WORD_LABEL),
        (SearchFlag::Regex, REGEX_LABEL),
    ] {
        let box_ = capsule.toggle(flag);
        let offered = look.offered.is_on(flag);
        // A toggle the host cannot honour is drawn at rest, at the same reveal
        // the head's spent navigation buttons wear, and never lit and never
        // hovered — the state says "there is nothing behind this here" without
        // moving anything.
        let on = offered && look.flags.is_on(flag);
        let hovered = offered && look.hover == Some(SearchElement::Toggle(flag));
        // `.sb-tg.on, .sb-tg.on:hover` — **the on state overrules the hover** (A34), so a switched
        // toggle does not change under the pointer. That is what makes "it is on" a fact you can
        // read while your hand is on it.
        if on {
            quads.extend(rounded_overlay_fill(
                box_,
                px(BUTTON_RADIUS_LOGICAL_PX),
                palette.accent,
                TOGGLE_ON_GROUND_ALPHA,
            ));
        } else if hovered {
            quads.extend(rounded_overlay_fill(
                box_,
                px(BUTTON_RADIUS_LOGICAL_PX),
                palette.menu_item_hover,
                1.0,
            ));
        }
        // **A toggle its host cannot honour is drawn in the ink this surface
        // uses for structure rather than for verbs** — the capsule's own
        // hairline colour. It keeps its box, so the two beside it do not move
        // under the pointer (this slice's own ruling about the navigation
        // buttons, one surface over), and it stops reading as something to
        // press.
        let ink = if !offered {
            palette.menu_item_unavailable_text
        } else if on {
            palette.accent
        } else if hovered {
            palette.menu_item_text_selected
        } else {
            palette.menu_item_hint_text
        };
        labels.push(ChromeLabel {
            mono: false,
            text: text.to_owned(),
            rect: box_,
            font_size_px: px(TOGGLE_FONT_LOGICAL_PX),
            color: ink,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(box_),
        });
        if flag == SearchFlag::Word {
            quads.push(word_underline(box_, text, scale, ink));
        }
    }

    // `.sb-sep`.
    quads.push(OverlayQuad {
        rect: capsule.separator,
        color: palette.menu_border,
        alpha: alpha(palette.menu_border_alpha),
    });

    for (element, box_, mark) in [
        (
            SearchElement::Previous,
            capsule.previous,
            // `.sb-nav[data-d="-1"] svg { transform: rotate(180deg) }` — one chevron, turned.
            ChromeMark::Chevron {
                turned_degrees: 180,
            },
        ),
        (
            SearchElement::Next,
            capsule.next,
            ChromeMark::Chevron { turned_degrees: 0 },
        ),
        (SearchElement::Close, capsule.close, ChromeMark::TabClose),
    ] {
        let hovered = look.hover == Some(element);
        if hovered {
            quads.extend(rounded_overlay_fill(
                box_,
                px(BUTTON_RADIUS_LOGICAL_PX),
                palette.menu_item_hover,
                1.0,
            ));
        }
        sprites.push(ChromeSprite::new(
            mark,
            centred(box_, px(BUTTON_GLYPH_LOGICAL_PX)),
            if hovered {
                palette.menu_item_text_selected
            } else {
                palette.menu_item_text
            },
        ));
    }

    OverlayLayer {
        quads,
        labels,
        sprites,
        ..OverlayLayer::default()
    }
}

/// `ab`'s underline: one physical pixel under the label's own two letters (D-12/D-15).
///
/// It spans the *label*, not the button — `text-decoration` underlines the text — so it is as wide
/// as the two characters are and is centred with them. Two letters at eleven pixels is about
/// thirteen; the width is taken from the box's own padding rather than measured, because the label
/// is centred in a box whose padding is declared and a measurement here would be a second answer to
/// a question the layout has already given one to.
fn word_underline(box_: [f32; 4], text: &str, scale: f32, ink: [u8; 3]) -> OverlayQuad {
    let px = |logical: f32| logical * scale;
    let inset = px(TOGGLE_PADDING_X_LOGICAL_PX);
    let thickness = px(1.0).round().max(1.0);
    // The baseline's own box is the label's rect; the offset hangs the rule below the glyphs the
    // way `text-underline-offset` does, and the label's box bottom is where the descent ends.
    let middle = (box_[1] + box_[3]) / 2.0;
    let half = px(TOGGLE_FONT_LOGICAL_PX) / 2.0;
    let y = (middle + half + px(WORD_UNDERLINE_OFFSET_LOGICAL_PX)).round();
    debug_assert_eq!(text.chars().count(), 2, "the whole-word toggle reads `ab`");
    OverlayQuad {
        rect: [box_[0] + inset, y, box_[2] - inset, y + thickness],
        color: ink,
        alpha: 1.0,
    }
}

/// A box of `size` centred in `rect`.
fn centred(rect: [f32; 4], size: f32) -> [f32; 4] {
    let x = (rect[0] + (rect[2] - rect[0] - size) / 2.0).round();
    let y = (rect[1] + (rect[3] - rect[1] - size) / 2.0).round();
    [x, y, x + size, y + size]
}

/// What one element's tip says (B66's `title=` attributes, quoted).
#[must_use]
pub fn tip_text(element: SearchElement) -> &'static str {
    match element {
        SearchElement::Toggle(SearchFlag::Case) => case_tip(),
        SearchElement::Toggle(SearchFlag::Word) => word_tip(),
        SearchElement::Toggle(SearchFlag::Regex) => regex_tip(),
        SearchElement::Previous => previous_tip(),
        SearchElement::Next => next_tip(),
        SearchElement::Close => close_tip(),
        SearchElement::Field | SearchElement::Body => "",
    }
}

/// Where a viewport has to stand for `row_height` at `row_top` to land at a third of the way down a
/// pane `pane_height` tall — the `local_offset` of the anchored scroll.
///
/// Negative, because `scroll_y = anchor_y + local_offset` and lifting the viewport's top *above*
/// the anchor is what puts the anchor further down the pane.
#[must_use]
pub fn landing_offset_subpixels(pane_height_subpixels: i64) -> i64 {
    -((pane_height_subpixels as f64 * f64::from(HIT_LANDING_FRACTION)) as i64)
}

/// Whether a row that the frame is showing stands **completely** inside the pane.
///
/// The prototype's own test (B54): *"scroll only when the hit is off screen — typing toward a
/// visible match must not yank the viewport"*, with visibility meaning the whole line fits. A row
/// half off the bottom is a row you cannot read, so it counts as off screen and is scrolled to.
#[must_use]
pub fn row_is_wholly_visible(top_subpixels: i64, height_subpixels: i64, pane_height: i64) -> bool {
    top_subpixels >= 0 && top_subpixels.saturating_add(height_subpixels) <= pane_height
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_doc::GridGeneration;
    use bt_transcript::{CapturedRow, TranscriptStore};
    use std::num::NonZeroUsize;
    use std::time::Instant;

    const SCALE: f32 = 1.0;
    /// A pane 900 wide and 600 tall, standing at the window's origin.
    const PANE: [f32; 4] = [0.0, 0.0, 900.0, 600.0];

    fn store(lines: &[&str]) -> TranscriptStore {
        let mut store = TranscriptStore::new(NonZeroUsize::new(200_000).unwrap());
        for line in lines {
            store.capture(CapturedRow::plain(line, false));
        }
        store
    }

    fn engine_for(text: &str, flags: SearchFlags) -> CompiledSearch {
        engine(flags, text).expect("the query under test compiles")
    }

    /// A grid row the way a real terminal captures one: **a wide glyph occupies two cells**, its
    /// own and a spacer carrying no text. `CapturedRow::plain` is a convenience for tests over
    /// narrow text and does not model that, so anything asserting about columns builds its own.
    fn wide_row(text: &str) -> Vec<bt_transcript::CapturedCell> {
        let mut cells = Vec::new();
        for cluster in bt_unicode::graphemes(text) {
            cells.push(bt_transcript::CapturedCell::plain(cluster));
            if bt_unicode::cluster_width(cluster) == 2 {
                cells.push(bt_transcript::CapturedCell {
                    wide_spacer: true,
                    ..bt_transcript::CapturedCell::default()
                });
            }
        }
        cells
    }

    fn hits_over(lines: &[&str], text: &str) -> Vec<Hit> {
        let store = store(lines);
        scan_history(&engine_for(text, SearchFlags::default()), &store)
    }

    /// A state with `text` typed into it and a scan already installed over `lines`.
    fn searching(lines: &[&str], text: &str) -> SearchState {
        let mut state = SearchState::default();
        state.open(SeatId(1));
        for character in text.chars() {
            state.field_mut().insert(&character.to_string());
        }
        let hits = hits_over(lines, text);
        state.install(hits, None, false, None);
        state
    }

    // ── the counter's four states (B51) ─────────────────────────────────────

    /// PIN (B51) — **the counter says one of exactly four things**, and the prototype's own
    /// literals are all four.
    ///
    /// The one worth naming is the third: no results reads `0/0` and **not** "No results". It is
    /// the same shape as the answer beside it, so the eye reads a pair of numbers that went to zero
    /// rather than a box that changed what kind of thing it says.
    ///
    /// MUTATIONS:
    /// (1) make the empty query read `0/0` — the first assertion goes red, and a reader who has
    ///     typed nothing is told they have found nothing;
    /// (2) collapse the broken and empty-result states into one — the second goes red and a bad
    ///     pattern becomes indistinguishable from a good one with no hits.
    #[test]
    fn the_counter_says_nothing_a_dash_a_pair_or_a_zero_pair() {
        assert_eq!(counter_text(true, false, 0, None), "");
        assert_eq!(counter_text(false, true, 0, None), "\u{2014}");
        assert_eq!(counter_text(false, false, 17, Some(2)), "3/17");
        assert_eq!(counter_text(false, false, 0, None), "0/0");
        // A broken pattern wins over a stale hit set: the field is red and the box says so.
        assert_eq!(counter_text(false, true, 4, Some(0)), "\u{2014}");
    }

    /// The four states again, reached the way a reader reaches them rather than by calling the
    /// formatter — so the wiring between the engine, the state and the box is what is pinned.
    #[test]
    fn a_state_reports_each_of_the_four_counter_states_as_it_is_typed_into() {
        let lines = ["alpha beta", "gamma"];
        let mut state = SearchState::default();
        state.open(SeatId(1));
        assert_eq!(state.counter(), "", "nothing typed is not nothing found");

        state.field_mut().insert("beta");
        state.install(hits_over(&lines, "beta"), None, false, None);
        assert_eq!(state.counter(), "1/1");

        state.field_mut().clear();
        state.field_mut().insert("zeta");
        state.install(hits_over(&lines, "zeta"), None, false, None);
        assert_eq!(state.counter(), "0/0");

        state.install(Vec::new(), Some("unclosed group".to_owned()), false, None);
        assert_eq!(state.counter(), "\u{2014}");
        assert_eq!(state.error(), Some("unclosed group"));
    }

    // ── the engine, through the capsule's own toggles ───────────────────────

    /// PIN (B48, user bug 2026-07-18) — **`m` on a line of eight `m`s counts eight.**
    ///
    /// The bug was a count of 23 on a screen showing 8, because the DOM held the same text twice
    /// over. The native answer cannot have that shape at all — the scan sees one logical line's
    /// source and nothing else — and this is the assertion that says so.
    ///
    /// MUTATION: scan the frame's cells instead of the transcript's lines and the number changes
    /// with the eye toggle, which is the whole of what this forbids.
    #[test]
    fn eight_ms_on_a_line_count_eight() {
        assert_eq!(hits_over(&["mmmmmmmm"], "m").len(), 8);
        assert_eq!(hits_over(&["m m m", "mm"], "m").len(), 5);
    }

    /// PIN (B49) — **a pattern that can match nothing terminates.**
    ///
    /// `a*` matches the empty string at every boundary, and a scan that reported those would paint
    /// a zero-width mark on every character in the transcript and count them all. The engine skips
    /// them; this is the app-level proof that the skip survives the trip through `scan_history`.
    #[test]
    fn a_star_completes_and_reports_only_the_runs_it_found() {
        let flags = SearchFlags {
            regex: true,
            ..SearchFlags::default()
        };
        let store = store(&["banana", "cherry"]);
        let hits = scan_history(&engine_for("a*", flags), &store);
        assert_eq!(
            hits.len(),
            3,
            "the three runs of `a` in `banana`, and nothing at all for the empty matches"
        );
        assert!(hits.iter().all(|hit| hit.end > hit.start));
    }

    /// PIN — **the three keyboard toggles are VS Code's, and they are the only three.**
    ///
    /// The mock-up gives the toggles a click and no key at all (B74), so this is the one place in
    /// the capsule where the reference product supplies the rule instead of the prototype. Folded
    /// for case, so `Alt+Shift+W` is still whole-word.
    ///
    /// MUTATION: add a fourth letter and the last assertion goes red — a capsule that answered
    /// `Alt+A` would be quietly claiming a chord nobody ruled.
    #[test]
    fn alt_c_w_and_r_are_the_three_toggles_and_nothing_else_is() {
        assert_eq!(toggle_for_letter("c"), Some(SearchFlag::Case));
        assert_eq!(toggle_for_letter("w"), Some(SearchFlag::Word));
        assert_eq!(toggle_for_letter("r"), Some(SearchFlag::Regex));
        assert_eq!(toggle_for_letter("C"), Some(SearchFlag::Case));
        assert_eq!(toggle_for_letter("W"), Some(SearchFlag::Word));
        assert_eq!(toggle_for_letter("R"), Some(SearchFlag::Regex));
        for letter in ('a'..='z').chain('A'..='Z') {
            let expected = matches!(letter, 'c' | 'C' | 'w' | 'W' | 'r' | 'R');
            assert_eq!(
                toggle_for_letter(&letter.to_string()).is_some(),
                expected,
                "Alt+{letter}"
            );
        }
    }

    /// The three toggles do what they say, and default to off (B43, D-4).
    #[test]
    fn the_three_toggles_default_off_and_each_changes_the_answer() {
        let flags = SearchFlags::default();
        assert!(!flags.case_sensitive && !flags.whole_word && !flags.regex);

        let lines = ["Cat concat cat."];
        let store = store(&lines);
        assert_eq!(scan_history(&engine_for("cat", flags), &store).len(), 3);

        let mut cased = flags;
        cased.toggle(SearchFlag::Case);
        assert_eq!(scan_history(&engine_for("cat", cased), &store).len(), 2);

        let mut whole = flags;
        whole.toggle(SearchFlag::Word);
        assert_eq!(scan_history(&engine_for("cat", whole), &store).len(), 2);

        let mut regex = flags;
        regex.toggle(SearchFlag::Regex);
        assert_eq!(scan_history(&engine_for("c.t", regex), &store).len(), 3);
        assert!(
            engine(flags, "c.t").is_ok_and(|compiled| compiled.pattern().contains("c\\.t")),
            "with the toggle off the dot is a dot"
        );
    }

    /// PIN (B62, D-4) — **closing keeps the query and the toggles; nothing is persisted.**
    ///
    /// `close` empties the seat, the hits and the current match, and touches neither the field nor
    /// the flags — which is what makes `Ctrl+F` after a tab switch a continuation. The second half
    /// of D-4 (not written to disk) is a fact about `bt-persist`, asserted by
    /// `nothing_about_the_search_is_written_to_the_session_file` in `main.rs`.
    ///
    /// MUTATION: clear the field in `close` and a reader who pressed Esc loses what they typed.
    #[test]
    fn closing_the_capsule_keeps_the_query_and_the_toggles_for_the_next_time() {
        let mut state = searching(&["alpha", "beta alpha"], "alpha");
        state.flags_mut().toggle(SearchFlag::Case);
        assert_eq!(state.hits().len(), 2);

        assert!(state.close(), "it was open");
        assert!(!state.is_open());
        assert!(state.hits().is_empty(), "the hit set goes with the capsule");
        assert!(state.current().is_none());
        assert_eq!(state.query(), "alpha", "the query stays");
        assert!(state.flags().case_sensitive, "and so does the toggle");

        assert!(!state.close(), "a second Esc has nothing here to close");

        state.open(SeatId(1));
        assert_eq!(state.query(), "alpha");
        assert!(state.flags().case_sensitive);
        assert!(state.is_focused());
        assert_eq!(
            state.field().selection(),
            0..5,
            "re-opening selects it: type to replace, Enter to reuse (B76)"
        );
    }

    /// PIN (D-4) — **the second half of "remembered": remembered for the process, and nowhere
    /// else.**
    ///
    /// The test above says the query and the toggles survive a close. This one says they do not
    /// survive the window, and it says it about the *schema* rather than about a code path: a
    /// session document that carried a search would make a query somebody typed on Tuesday into a
    /// preference nobody set, and would put a `.*` toggle on a window that opened this morning.
    ///
    /// Written against the serialized form, because that is the thing the file actually is.
    /// `grep -rn search crates/bt-persist` is empty today; this is what keeps it empty.
    #[test]
    fn nothing_about_the_search_is_written_to_the_session_file() {
        let document = serde_json::to_string(&bt_persist::SessionV1::default())
            .expect("the session schema serializes");
        for word in ["search", "srch", "query", "whole_word", "case_sensitive"] {
            assert!(
                !document.contains(word),
                "the session schema mentions `{word}`: a search is a thing you are doing,                  not a thing this window is"
            );
        }
    }

    /// PIN — **the capsule being up and the capsule holding the keyboard are two facts** (B81).
    ///
    /// The second stance the prototype names — *"search open, hands back on the terminal"* — is
    /// what `F3` exists for and what a click in the pane produces. So a blur leaves the capsule
    /// standing, its hits lit and its count true, while the shell takes the keys back.
    ///
    /// MUTATION: make `blur` close the capsule and a click in the pane throws the search away,
    /// which is a popup and not a staying state.
    #[test]
    fn a_blur_hands_the_keyboard_back_and_leaves_the_capsule_standing() {
        let mut state = searching(&["hit one", "hit two"], "hit");
        assert!(state.is_open() && state.is_focused());

        state.blur();
        assert!(state.is_open(), "the capsule stays up");
        assert!(!state.is_focused(), "and the shell has the keyboard");
        assert_eq!(state.hits().len(), 2, "the hits stay lit");
        assert_eq!(state.counter(), "1/2", "and the count stays true");

        // `F3` walks from that stance, which is the whole point of it.
        assert!(state.step(true).is_some());
        assert_eq!(state.counter(), "2/2");

        state.focus();
        assert!(state.is_focused());

        // A closed capsule is never focused, whatever was true a moment before.
        state.close();
        assert!(!state.is_focused());
        assert!(!state.is_open());
    }

    // ── the walk ────────────────────────────────────────────────────────────

    /// PIN (B61) — **stepping is a ring**, deliberately unlike the command rail's walk.
    ///
    /// A rail is a history with two ends and stops at them; a search is a set of places in a text,
    /// and stepping off the last one means "start again".
    ///
    /// MUTATION: clamp instead of wrapping and the last `Enter` of a search does nothing, which is
    /// the one outcome that reads as a broken key.
    #[test]
    fn stepping_wraps_at_both_ends() {
        let mut state = searching(&["one hit", "two hit", "three hit"], "hit");
        assert_eq!(state.hits().len(), 3);
        let at = |state: &SearchState| {
            state
                .hits()
                .iter()
                .position(|hit| state.current().is_some_and(|current| current.is(hit)))
        };
        assert_eq!(at(&state), Some(0));
        state.step(true);
        state.step(true);
        assert_eq!(at(&state), Some(2));
        state.step(true);
        assert_eq!(at(&state), Some(0), "off the end is back to the beginning");
        state.step(false);
        assert_eq!(
            at(&state),
            Some(2),
            "and backwards off the front is the end"
        );
    }

    /// Stepping with nothing found does nothing at all — and says so, so the caller does not
    /// scroll to a match that is not there.
    #[test]
    fn stepping_an_empty_hit_set_answers_nothing() {
        let mut state = searching(&["alpha"], "zeta");
        assert!(state.step(true).is_none());
        assert!(state.step(false).is_none());
        assert!(state.current().is_none());
    }

    /// PIN (R2) — **the current match survives new output.**
    ///
    /// Search is live: while the capsule is open the shell goes on printing, so every rebuild
    /// renumbers the hit set. The match the reader is standing on is found again **by identity** —
    /// the same range of the same line — and not by index, which has certainly moved.
    ///
    /// MUTATIONS:
    /// (1) keep the index instead of the identity — the walk jumps to a different hit every time a
    ///     line lands above it;
    /// (2) drop `keep_current` and every line of output throws the reader back to the top of the
    ///     scrollback.
    #[test]
    fn the_current_match_keeps_its_place_when_output_arrives_above_it() {
        let mut transcript = store(&["hit one", "hit two"]);
        let compiled = engine_for("hit", SearchFlags::default());
        let mut state = SearchState::default();
        state.open(SeatId(1));
        state.field_mut().insert("hit");
        state.install(scan_history(&compiled, &transcript), None, false, None);
        state.step(true);
        let standing = state.current().cloned().expect("a current match");
        assert_eq!(state.counter(), "2/2");

        // The shell goes on printing into the *same* transcript, which is the only way a hit set
        // can actually grow: the line ids the reader is standing on do not move, and the count
        // under them does.
        transcript.capture(CapturedRow::plain("hit three", false));
        transcript.capture(CapturedRow::plain("hit four", false));
        state.install(scan_history(&compiled, &transcript), None, true, None);
        assert_eq!(state.hits().len(), 4);
        assert!(
            state.current().is_some_and(|hit| hit.is(&standing)),
            "the same range of the same line, found again by identity"
        );
        assert_eq!(
            state.counter(),
            "2/4",
            "the reader has not moved; the transcript has grown under them"
        );

        // And the walk goes on from where they are rather than from where the rebuild put them.
        state.step(true);
        assert_eq!(state.counter(), "3/4");
    }

    /// The other half of the same rule: when the line under the caret is gone, the walk moves
    /// **forwards** to the nearest surviving hit rather than back to the beginning.
    #[test]
    fn a_current_match_whose_line_was_evicted_falls_to_the_next_one() {
        let mut state = searching(&["hit one", "hit two", "hit three"], "hit");
        state.step(true);
        let standing = state.current().cloned().expect("a current match");

        // Everything up to and including the standing line is dropped, as a quota eviction drops
        // the oldest lines: the survivors are the ones after it.
        let hits: Vec<Hit> = state
            .hits()
            .iter()
            .filter(|hit| hit.line > standing.line)
            .cloned()
            .collect();
        state.install(hits, None, true, None);
        assert_eq!(state.counter(), "1/1", "the nearest hit after the lost one");
    }

    /// PIN (B58) — **a rebuild the reader caused starts from where the eye is.**
    ///
    /// *"Start from where the eye is: the first match at or below the viewport top."* Typing
    /// another letter into a query must not throw the view to the top of a hundred thousand lines
    /// of scrollback.
    #[test]
    fn a_fresh_query_starts_at_the_first_match_below_the_viewport_top() {
        let lines = ["hit a", "filler", "hit b", "filler", "hit c"];
        let hits = hits_over(&lines, "hit");
        let mut state = SearchState::default();
        state.open(SeatId(1));
        state.field_mut().insert("hit");
        // The viewport is showing the third line, so the answer is the hit on it and not the one
        // two lines above.
        let viewport = hits[1].anchor.clone();
        state.install(hits, None, false, Some(&viewport));
        assert_eq!(state.counter(), "2/3");
        // With the pane at the live bottom there is no anchor, and every hit is "at or below", so
        // the answer is the first.
        let hits = hits_over(&lines, "hit");
        state.install(hits, None, false, None);
        assert_eq!(state.counter(), "1/3");
    }

    // ── what is searched ────────────────────────────────────────────────────

    /// A hit is addressed in **its own plane's unit**: graphemes in history, columns on the grid.
    ///
    /// The two are not the same number the moment a wide glyph is on the line, which is what this
    /// pins: `中文` is two graphemes and four columns, so a hit after it starts at grapheme 2 in
    /// the transcript and at column 4 on the screen. Getting this wrong puts every highlight on a
    /// CJK line half a glyph out.
    #[test]
    fn a_hit_is_measured_in_graphemes_in_history_and_in_columns_on_the_grid() {
        let store = store(&["\u{4e2d}\u{6587}ok"]);
        let hits = scan_history(&engine_for("ok", SearchFlags::default()), &store);
        assert_eq!((hits[0].start, hits[0].end), (2, 4));

        let row = live_row(0, &wide_row("\u{4e2d}\u{6587}ok"));
        let hits = scan_volatile(
            &engine_for("ok", SearchFlags::default()),
            &TranscriptStore::new(NonZeroUsize::new(8).unwrap()),
            std::slice::from_ref(&row),
            GridGeneration(1),
        );
        assert_eq!(
            (hits[0].start, hits[0].end),
            (4, 6),
            "the grid counts columns, and a wide glyph takes two of them"
        );
    }

    /// PIN (R7) — **the live grid is searched too.**
    ///
    /// *"The word is on my screen and search cannot find it"* is the failure this exists to
    /// prevent: a command still being typed has not scrolled out, so it is in no transcript at all.
    ///
    /// The order is the document's own (§3.2): history, then staging, then the grid.
    #[test]
    fn the_screen_is_searched_beside_the_history_and_comes_after_it() {
        let store = store(&["error in history"]);
        let rows = [
            live_row(0, &CapturedRow::plain("error on screen", false).cells),
            live_row(1, &CapturedRow::plain("clean", false).cells),
        ];
        let compiled = engine_for("error", SearchFlags::default());
        let mut hits = scan_history(&compiled, &store);
        hits.extend(scan_volatile(&compiled, &store, &rows, GridGeneration(1)));
        assert_eq!(hits.len(), 2);
        assert!(matches!(hits[0].line, SearchLine::History(_)));
        assert_eq!(hits[1].line, SearchLine::Live { row: 0 });
    }

    // ── the capsule's geometry ──────────────────────────────────────────────

    /// PIN (A25, A26) — **the capsule rides the pane's top-right, and clears the head when there
    /// is one.**
    ///
    /// The bare top is the mock-up's `10px`. The one with a head is *derived*: the head's own
    /// `content_bottom` plus eight, which comes out at the mock-up's 38 for a 30-pixel head and
    /// follows the head if it ever changes.
    ///
    /// MUTATION: write `38.0` instead and a head of any other height leaves the capsule sitting on
    /// top of it.
    #[test]
    fn the_capsule_hangs_ten_below_the_pane_and_eight_below_a_head() {
        let bare = lay_out(PANE, None, SCALE, 36.0);
        assert_eq!(bare.frame[1], 10.0);
        // 30px head, less its one-pixel hairline, is the content bottom a real head reports.
        let with_head = lay_out(PANE, Some(29.0), SCALE, 36.0);
        assert_eq!(with_head.frame[1], 37.0);
        assert_eq!(
            with_head.frame[2], bare.frame[2],
            "a head moves it down and never sideways"
        );
    }

    /// PIN (A25, R4, R5) — **the capsule clears the scroll lane and the rail's own box.**
    ///
    /// The mock-up writes `right: 28px`, which is 8 of reserved scroll lane, 3 of gap, the rail's
    /// 15-pixel resting box and 2 of air. Every term but the last is read from the constant that
    /// owns it, so the day the lane's width is settled the capsule moves with the rail rather than
    /// landing on top of it — which is the 2026-07-18 accident, one instrument along.
    #[test]
    fn the_capsule_clears_the_scroll_lane_and_the_rail() {
        assert_eq!(
            right_inset_logical_px(),
            28.0,
            "the mock-up's own number, arrived at by adding up what is actually there"
        );
        let capsule = lay_out(PANE, None, SCALE, 36.0);
        assert_eq!(capsule.frame[2], PANE[2] - 28.0);
        assert!(
            capsule.frame[0] > PANE[0],
            "and a 900px pane has room for the whole of it"
        );
    }

    /// PIN — **a pane too narrow for the capsule gets the whole capsule anyway**, hanging off its
    /// left edge.
    ///
    /// The first cut clamped the left edge to the pane, which shortens the *frame* without
    /// shortening the row of controls inside it: a real 300-pixel pane drew the capsule with its
    /// own cross outside its own rounded box (photographed 2026-08-16). The stylesheet has no
    /// clamp — an absolutely positioned box inside a `position: relative` pane with nothing hiding
    /// the overflow simply hangs off — and neither does this.
    ///
    /// MUTATION: clamp the left edge to the pane again and the last assertion goes red, which is
    /// the bug arriving back.
    #[test]
    fn a_pane_too_narrow_for_the_capsule_gives_the_field_and_keeps_the_controls() {
        let wide = lay_out(PANE, None, SCALE, 36.0);
        assert_eq!(
            wide.field[2] - wide.field[0],
            FIELD_WIDTH_LOGICAL_PX,
            "with room, the field is its declared width"
        );

        let narrow = [0.0, 0.0, 300.0, 400.0];
        let capsule = lay_out(narrow, None, SCALE, 36.0);
        assert!(
            capsule.field[2] - capsule.field[0] < FIELD_WIDTH_LOGICAL_PX,
            "the field is what gives"
        );
        assert!(capsule.field[2] - capsule.field[0] >= FIELD_MIN_WIDTH_LOGICAL_PX);
        assert!(
            capsule.frame[0] >= narrow[0],
            "and the whole capsule fits inside a 300px pane"
        );
        // Whatever it had to give, the box is still drawn around every one of its controls — which
        // is the half the first cut got wrong.
        assert!(capsule.field[0] >= capsule.frame[0]);
        assert!(capsule.close[2] <= capsule.frame[2]);

        // Past the floor it hangs off the left edge whole, exactly as an absolutely positioned box
        // in a `position: relative` parent does, rather than coming apart.
        let sliver = lay_out([0.0, 0.0, 150.0, 400.0], None, SCALE, 36.0);
        assert!(sliver.frame[0] < 0.0);
        assert_eq!(
            sliver.field[2] - sliver.field[0],
            FIELD_MIN_WIDTH_LOGICAL_PX
        );
        assert!(sliver.close[2] <= sliver.frame[2]);
    }

    /// The nine children stand in the mock-up's order, left to right, without overlapping.
    #[test]
    fn the_nine_elements_stand_in_the_mock_ups_own_order() {
        let capsule = lay_out(PANE, None, SCALE, 36.0);
        let boxes = [
            capsule.field,
            capsule.counter,
            capsule.case,
            capsule.word,
            capsule.regex,
            capsule.separator,
            capsule.previous,
            capsule.next,
            capsule.close,
        ];
        for pair in boxes.windows(2) {
            assert!(
                pair[0][2] <= pair[1][0],
                "{:?} runs into {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(capsule.frame[0] <= boxes[0][0]);
        assert!(boxes[8][2] <= capsule.frame[2]);
        assert_eq!(capsule.field[2] - capsule.field[0], FIELD_WIDTH_LOGICAL_PX);
    }

    /// PIN (user report, 2026-08-17) — **the caret the IME is given is inside the
    /// field, and it walks with what has been typed in front of it.**
    ///
    /// The bug was not a wrong rectangle, it was *no* rectangle: the capsule
    /// published nothing, so a composition begun in the search field left the
    /// candidate list wherever the last caret this window published had been —
    /// the bottom-right of the window in the photograph. What the window hands
    /// `set_ime_cursor_area` is this line box, so it is asserted here, next to
    /// the layout that produces it and without a font or a GPU:
    ///
    /// - it stands **inside the field's own box**, on both axes, at every prefix
    ///   including one longer than the field;
    /// - it spans the field's whole **line** rather than the inset bar the reader
    ///   sees, because the candidate window is placed clear of what it is given
    ///   and half a field of clearance is not clearance;
    /// - it **moves right** as the caret byte moves along the query, which is the
    ///   half of the report that a fixed rectangle would still have failed.
    ///
    /// MUTATIONS: return the field box's left edge instead of the caret's and the
    /// walk assertion goes red; inset the top and bottom by
    /// `FIELD_CARET_INSET_LOGICAL_PX` and the line assertion does; drop the clamp
    /// and the overlong prefix escapes the field.
    #[test]
    fn the_field_gives_the_ime_a_caret_inside_itself_that_walks_with_the_query() {
        let capsule = lay_out(PANE, None, SCALE, 36.0);
        // A stand-in for the font: what the window passes is a measured width,
        // and every measured width is a non-negative number of pixels.
        let advance = 7.0;
        let inside = |line: [f32; 4]| {
            line[0] >= capsule.field[0]
                && line[2] <= capsule.field[2]
                && line[1] >= capsule.field[1]
                && line[3] <= capsule.field[3]
        };

        let empty = capsule.caret_line(0.0, SCALE);
        assert!(inside(empty), "{empty:?} escapes {:?}", capsule.field);
        assert_eq!(
            (empty[1], empty[3]),
            (capsule.field[1], capsule.field[3]),
            "the IME is told the line, not the bar drawn inside it"
        );
        assert!(empty[2] > empty[0], "a caret with no width is not a caret");

        let mut last = empty;
        for byte in 1..=4u32 {
            let line = capsule.caret_line(byte as f32 * advance, SCALE);
            assert!(inside(line), "{line:?} escapes {:?}", capsule.field);
            assert!(
                line[0] > last[0],
                "the caret stood still at byte {byte}: {line:?} after {last:?}"
            );
            assert_eq!(
                (line[1], line[3]),
                (empty[1], empty[3]),
                "a one-line field's caret moves along the line and never off it"
            );
            last = line;
        }

        // A query longer than its box scrolls under the field's own right
        // padding; the candidate list may not follow it out of the capsule.
        let overrun = capsule.caret_line(10_000.0, SCALE);
        assert!(
            inside(overrun),
            "{overrun:?} escapes {:?} — a caret past the end of the box is still \
             in the box",
            capsule.field
        );
    }

    /// Every control answers a press on itself, and the capsule's own padding answers as its body
    /// rather than falling through to the pane (B74).
    #[test]
    fn a_press_lands_on_the_control_it_looks_like_it_landed_on() {
        let capsule = lay_out(PANE, None, SCALE, 36.0);
        let middle = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        for (rect, expected) in [
            (capsule.field, SearchElement::Field),
            (capsule.case, SearchElement::Toggle(SearchFlag::Case)),
            (capsule.word, SearchElement::Toggle(SearchFlag::Word)),
            (capsule.regex, SearchElement::Toggle(SearchFlag::Regex)),
            (capsule.previous, SearchElement::Previous),
            (capsule.next, SearchElement::Next),
            (capsule.close, SearchElement::Close),
        ] {
            let (x, y) = middle(rect);
            assert_eq!(hit(&capsule, x, y), Some(expected));
        }
        assert_eq!(
            hit(&capsule, capsule.frame[0] + 1.0, capsule.frame[1] + 1.0),
            Some(SearchElement::Body),
            "the capsule is one control: even its padding is claimed"
        );
        assert_eq!(
            hit(&capsule, capsule.frame[0] - 1.0, capsule.frame[1]),
            None
        );
        assert_eq!(
            hit(&capsule, capsule.frame[2] + 1.0, capsule.frame[1]),
            None
        );
    }

    /// The capsule grows with its counter rather than clipping it, and never shrinks below the
    /// mock-up's `min-width` — so a walk from `9/12` to `10/12` does not make the box breathe.
    #[test]
    fn the_counter_box_never_shrinks_below_its_declared_minimum() {
        let narrow = lay_out(PANE, None, SCALE, 4.0);
        assert_eq!(
            narrow.counter[2] - narrow.counter[0],
            COUNTER_MIN_WIDTH_LOGICAL_PX
        );
        let wide = lay_out(PANE, None, SCALE, 90.0);
        assert_eq!(wide.counter[2] - wide.counter[0], 90.0);
        assert!(wide.frame[0] < narrow.frame[0], "the box grows leftward");
        assert_eq!(
            wide.frame[2], narrow.frame[2],
            "and its right edge is fixed"
        );
    }

    // ── the scroll ──────────────────────────────────────────────────────────

    /// PIN (B54) — **scroll only when the hit is off screen, and land it a third of the way down.**
    ///
    /// *"Typing toward a visible match must not yank the viewport."* A row that stands wholly
    /// inside the pane is a row the reader can already read; one hanging half off the bottom is
    /// not, so it counts as off screen.
    ///
    /// MUTATIONS:
    /// (1) scroll unconditionally — the view jumps on every keystroke of a query whose match is
    ///     already on the glass;
    /// (2) land at the top instead of a third down and a match arrives with no context above it,
    ///     which is what tells a rail's jump apart from a search's.
    #[test]
    fn a_visible_row_is_left_alone_and_an_invisible_one_lands_a_third_down() {
        let pane = 600 * bt_viewport::SUBPIXELS_PER_PX;
        let row = 20 * bt_viewport::SUBPIXELS_PER_PX;
        assert!(row_is_wholly_visible(0, row, pane));
        assert!(row_is_wholly_visible(pane - row, row, pane));
        assert!(
            !row_is_wholly_visible(pane - row / 2, row, pane),
            "half off the bottom is not readable"
        );
        assert!(!row_is_wholly_visible(-1, row, pane));

        assert_eq!(landing_offset_subpixels(pane), -(pane / 3));
        assert!(
            landing_offset_subpixels(pane) < 0,
            "a negative local offset lifts the viewport above the anchor, \
             which is what puts the anchor further down the pane"
        );
    }

    // ── perf ────────────────────────────────────────────────────────────────

    /// D-20's measurement, at the app level: how long a keystroke's worth of work takes over the
    /// spike's own 100,000-line ceiling.
    ///
    /// No assertion beyond completing. The number is the point — it is what the ticket asks to be
    /// reported, and it is what a future ruling on the scrollback ceiling will be argued from.
    /// Both halves of the split are timed, because the split is the design: the first is what a
    /// keystroke costs, the second is what every *frame* costs while the capsule is open.
    #[test]
    fn a_hundred_thousand_lines_rebuild_in_a_time_worth_printing() {
        let mut transcript = TranscriptStore::new(NonZeroUsize::new(200_000).unwrap());
        for index in 0..100_000u32 {
            transcript.capture(CapturedRow::plain(
                &format!("2026-08-16 12:00:00 worker {index} finished task in {index} ms with cat"),
                false,
            ));
        }
        let rows: Vec<LiveRow> = (0..40u32)
            .map(|row| {
                live_row(
                    row,
                    &CapturedRow::plain("a cat on the live grid", false).cells,
                )
            })
            .collect();
        let compiled = engine_for("cat", SearchFlags::default());

        let started = Instant::now();
        let history = scan_history(&compiled, &transcript);
        let history_elapsed = started.elapsed();

        let started = Instant::now();
        let volatile = scan_volatile(&compiled, &transcript, &rows, GridGeneration(1));
        let volatile_elapsed = started.elapsed();

        let mut state = SearchState::default();
        state.open(SeatId(1));
        state.field_mut().insert("cat");
        let mut hits = history.clone();
        hits.extend(volatile.iter().cloned());
        let total = hits.len();
        let started = Instant::now();
        state.install(hits, None, false, None);
        let install_elapsed = started.elapsed();

        assert_eq!(history.len(), 100_000);
        assert_eq!(volatile.len(), 40);
        assert_eq!(state.counter(), format!("1/{total}"));
        println!(
            "S3: 100k frozen lines -> {} hits in {history_elapsed:?}; \
             40 live rows -> {} hits in {volatile_elapsed:?}; \
             installing {total} hits (grouping + highlight index) in {install_elapsed:?}",
            history.len(),
            volatile.len(),
        );
    }
}

/// **The capsule over its second host** (§7.7 ②, W2 slice ④): a page answers one
/// of the three toggles and counts its own matches.
#[cfg(test)]
mod second_host_tests {
    use super::*;

    const SCALE: f32 = 1.0;
    const PANE: [f32; 4] = [0.0, 0.0, 900.0, 600.0];

    fn drawn(offered: SearchFlags, flags: SearchFlags) -> Vec<ChromeLabel> {
        let capsule = lay_out(PANE, None, SCALE, 40.0);
        let palette = bt_render::chrome_palette();
        build(
            &capsule,
            &CapsuleLook {
                text: "ripgrep",
                typed: true,
                caret_x: 20.0,
                focused: true,
                broken: false,
                counter: "1/4",
                flags,
                offered,
                hover: Some(SearchElement::Toggle(SearchFlag::Word)),
            },
            &palette,
            SCALE,
        )
        .labels
    }

    fn ink_of(labels: &[ChromeLabel], text: &str) -> [u8; 3] {
        labels
            .iter()
            .find(|label| label.text == text)
            .map(|label| label.color)
            .unwrap_or_else(|| panic!("the capsule draws {text:?}"))
    }

    /// PIN (§7.7 ②) — **a toggle its host cannot honour is drawn fainter than
    /// one it can, keeps its box, and does not light under the pointer.**
    ///
    /// `ICoreWebView2FindOptions` carries a find term, a case fold and a
    /// highlight-all; there is no word boundary and no pattern anywhere in that
    /// interface. Dimmed and inert rather than gone, which is this slice's own
    /// ruling about the three navigation buttons applied one surface over: a
    /// control that vanishes moves the ones beside it under the pointer.
    ///
    /// MUTATIONS:
    /// ① draw an un-offered toggle in `menu_border` — that is an alpha-blended
    ///    hairline colour whose opaque value is pure white, so the two that
    ///    cannot be pressed come out **brighter** than the one that can, which
    ///    is what the real window showed on 2026-08-22;
    /// ② let the hover light an un-offered toggle — the last assertion goes red
    ///    and a control that answers nothing lights up under the hand.
    #[test]
    fn a_toggle_its_host_cannot_honour_is_drawn_fainter_and_never_lights() {
        let palette = bt_render::chrome_palette();
        let all = SearchFlags {
            case_sensitive: true,
            whole_word: true,
            regex: true,
        };
        let page = SearchFlags {
            case_sensitive: true,
            whole_word: false,
            regex: false,
        };
        let nothing_on = SearchFlags::default();

        let on_a_page = drawn(page, nothing_on);
        assert_eq!(ink_of(&on_a_page, CASE_LABEL), palette.menu_item_hint_text);
        assert_eq!(
            ink_of(&on_a_page, WORD_LABEL),
            palette.menu_item_unavailable_text,
            "a word boundary is not a thing this host can be asked for"
        );
        assert_eq!(
            ink_of(&on_a_page, REGEX_LABEL),
            palette.menu_item_unavailable_text
        );
        // Fainter, and not merely different — this is the half the first draft
        // got backwards.
        let ground = palette.menu_surface;
        let distance = |ink: [u8; 3]| {
            (0..3)
                .map(|i| (i32::from(ink[i]) - i32::from(ground[i])).abs())
                .sum::<i32>()
        };
        assert!(
            distance(palette.menu_item_unavailable_text) < distance(palette.menu_item_hint_text),
            "an unavailable toggle must sit closer to its own ground than a resting one"
        );

        // On a terminal every one of the three is offered: the two the pointer
        // is not on rest in the hint ink, and the one it is on lights.
        let on_a_terminal = drawn(all, nothing_on);
        for label in [CASE_LABEL, REGEX_LABEL] {
            assert_eq!(
                ink_of(&on_a_terminal, label),
                palette.menu_item_hint_text,
                "{label} is offered on a terminal"
            );
        }
        assert_eq!(
            ink_of(&on_a_terminal, WORD_LABEL),
            palette.menu_item_text_selected,
            "and the one under the pointer lights"
        );
        // **The pointer is on `ab` in both draws.** On the page it changed
        // nothing, which is the second half of the ruling: a control that
        // answers nothing does not light under the hand.
        assert_eq!(
            ink_of(&on_a_page, WORD_LABEL),
            palette.menu_item_unavailable_text
        );
    }

    /// PIN (§7.7 ②) — **a host that counts its own matches has no count until it
    /// has answered, and `0/0` is not that.**
    ///
    /// The engine is asked when the reader asks — Enter, the walk — so between
    /// the keystroke and the answer there genuinely is no tally. `0/0` there
    /// would be the capsule inventing one.
    ///
    /// MUTATION: drop `counts_its_own` and the first assertion reads `0/0`,
    /// which is the capsule claiming a page has no matches before it has looked.
    #[test]
    fn a_host_that_counts_its_own_has_no_count_until_it_has_answered() {
        let mut state = SearchState::default();
        state.open(SeatId(2));
        state.set_counts_its_own(true);
        state.field_mut().insert("ripgrep");
        assert_eq!(state.counter(), "", "asked for, not yet answered");
        assert!(state.report_engine_matches(4, 1));
        assert_eq!(state.counter(), "1/4");
        // A term that moves takes the answer down with it.
        state.forget_engine_matches();
        assert_eq!(state.counter(), "");
        // And a terminal's capsule is untouched: it counts through its own hits,
        // so an empty hit set really is `0/0`.
        let mut terminal = SearchState::default();
        terminal.open(SeatId(1));
        terminal.field_mut().insert("ripgrep");
        assert_eq!(terminal.counter(), EMPTY_COUNT);
    }
}
