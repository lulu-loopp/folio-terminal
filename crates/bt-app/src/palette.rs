//! The command palette: a floating intent box over a window that stays visible.
//!
//! DESIGN.md §7.55. The surface is `design/ui-mockup.html` 2363-2394 (its style),
//! 4629-4636 (its shape) and 6677-6840 (its behaviour); this module is the port
//! of the last of those three, and of nothing else — the rectangles it draws and
//! the sections it draws them in live here, the five suppliers that fill those
//! sections live at their own sources and reach this module as already-built
//! rows.
//!
//! **It is chrome, not content** (§7.x 压测补遗 ③): the palette is a way of
//! looking at the model, so it never becomes a seat, never enters the content
//! matrix, and never survives a restart.

/// One run of the score: a query character landing directly after the previous
/// one is worth twice its place in the run, so `nt` inside `New tab` beats the
/// same two letters scattered across it.
///
/// DESIGN.md §7.55 ①; `design/ui-mockup.html:6688`.
const RUN_WEIGHT: f32 = 2.0;

/// A query character landing at the start of a word is worth this much on top.
///
/// DESIGN.md §7.55 ①; `design/ui-mockup.html:6690`.
const WORD_START_BONUS: f32 = 3.0;

/// What each skipped character costs.
///
/// DESIGN.md §7.55 ①; `design/ui-mockup.html:6691`.
const GAP_COST_PER_CHAR: f32 = 0.15;

/// A gap stops getting worse after this many characters.
///
/// Without the cap a query that matches late in a long label would be beaten by
/// the label's own length rather than by anything about the match, and the file
/// section's labels are the longest strings in the list.
///
/// DESIGN.md §7.55 ①; `design/ui-mockup.html:6691`.
const GAP_COST_CAP: usize = 8;

/// The characters a word may start after, beyond whitespace.
///
/// The mock-up's `[\s\-_:·]`, with `\s` read as it reads in a regular
/// expression — every whitespace character, not only the space — so that a
/// label broken by a tab or a non-breaking space is scored the way the same
/// label broken by a space is.
const WORD_START_MARKS: [char; 4] = ['-', '_', ':', '·'];

/// A query matched against one candidate string.
#[derive(Clone, Debug, PartialEq)]
pub struct FuzzyMatch {
    /// Higher is better. Comparable only against other matches of the **same**
    /// query: the run and word-start terms both grow with the query's length,
    /// so two queries' scores are two different scales.
    pub score: f32,
    /// Where each query character landed, as **character** offsets into the
    /// candidate — the same unit the painter counts its glyphs in, so a label
    /// with a Chinese character in it highlights the character the match was on
    /// rather than the third byte of it.
    pub hits: Vec<usize>,
}

/// Fold one character for a case-insensitive comparison, **one character to one
/// character**.
///
/// [`char::to_lowercase`] is allowed to return more than one character (`İ`
/// lowercases to `i` followed by a combining dot), and this function's answer
/// addresses positions in the original string: a fold that changed the length
/// would make [`FuzzyMatch::hits`] point at the wrong glyph in exactly the
/// strings it was added for. So the first character of the mapping is the fold,
/// which is what a UTF-16 `toLowerCase` comparison in the mock-up amounts to for
/// every character that has a single-character lowering, and is an honest
/// approximation for the handful that do not.
fn fold(ch: char) -> char {
    // `to_lowercase` yields at least one character for every input, so the
    // fallback is unreachable rather than a guess; it is written as a default
    // instead of a panic because a query is user input.
    ch.to_lowercase().next().unwrap_or(ch)
}

/// Whether a character starts a word — i.e. whether the character **before** it
/// was a break.
fn is_break(ch: char) -> bool {
    ch.is_whitespace() || WORD_START_MARKS.contains(&ch)
}

/// Score `query` against `text`, or `None` when `text` does not contain the
/// query's characters in order.
///
/// Case-insensitive, subsequence-based, and a pure function of its two
/// arguments — the palette's ordering has to be reproducible from a query and a
/// list, because every gate that says "this section is ordered by score" is
/// otherwise measuring the order the suppliers happened to hand rows over in.
///
/// An empty query matches everything with a score of zero and no hits, which is
/// what makes the empty-query shape (DESIGN.md §7.55 ②) a case of the same code
/// path rather than a branch beside it.
#[must_use]
pub fn fuzzy_score(query: &str, text: &str) -> Option<FuzzyMatch> {
    let needle: Vec<char> = query.chars().map(fold).collect();
    if needle.is_empty() {
        return Some(FuzzyMatch {
            score: 0.0,
            hits: Vec::new(),
        });
    }
    let hay: Vec<char> = text.chars().map(fold).collect();

    let mut from = 0usize;
    let mut score = 0.0f32;
    let mut run = 0usize;
    let mut hits = Vec::with_capacity(needle.len());

    for want in needle {
        let found = hay[from..].iter().position(|ch| *ch == want)? + from;

        // A run is "landed exactly where the last one left off", which is why
        // it is compared against `from` and not against the previous hit: the
        // first character of the query starts a run when it lands at 0.
        run = if found == from { run + 1 } else { 1 };

        #[expect(
            clippy::cast_precision_loss,
            reason = "a run is bounded by the query's length and a gap by GAP_COST_CAP; \
                      neither reaches the f32 integer limit"
        )]
        let run_term = run as f32 * RUN_WEIGHT;
        let word_start = if found == 0 {
            true
        } else {
            // `found` is a character index into `hay`, so the character before
            // it is `found - 1` — there is no byte arithmetic to get wrong.
            is_break(hay[found - 1])
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "the gap is capped at GAP_COST_CAP characters"
        )]
        let gap_term = (found - from).min(GAP_COST_CAP) as f32 * GAP_COST_PER_CHAR;

        score += run_term + if word_start { WORD_START_BONUS } else { 0.0 } - gap_term;
        hits.push(found);
        from = found + 1;
    }

    Some(FuzzyMatch { score, hits })
}

#[cfg(test)]
mod fuzzy_tests {
    use super::{FuzzyMatch, fuzzy_score};

    fn score(query: &str, text: &str) -> f32 {
        fuzzy_score(query, text)
            .unwrap_or_else(|| panic!("{query:?} should match {text:?}"))
            .score
    }

    fn hits(query: &str, text: &str) -> Vec<usize> {
        fuzzy_score(query, text)
            .unwrap_or_else(|| panic!("{query:?} should match {text:?}"))
            .hits
    }

    /// PIN — **every query character must appear, in order**.
    ///
    /// The subsequence rule is the whole of what makes the list a filter rather
    /// than a ranking of everything: a row the query cannot be spelled out of
    /// is absent, not last.
    ///
    /// MUTATIONS:
    /// (1) search the whole haystack instead of `hay[from..]` — `bat` starts
    ///     matching `tab` and this goes green where it should be red;
    /// (2) return `Some` with a zero score instead of `None` on a miss — the
    ///     first two assertions go red.
    #[test]
    fn a_query_that_is_not_a_subsequence_does_not_match() {
        assert_eq!(fuzzy_score("bat", "tab"), None);
        assert_eq!(fuzzy_score("xyz", "New tab"), None);
        assert!(fuzzy_score("nt", "New tab").is_some());
        assert!(fuzzy_score("tab", "New tab").is_some());
    }

    /// PIN — **an empty query matches everything, with nothing highlighted**.
    ///
    /// This is what lets the empty-query shape be the same code path as any
    /// other: the sections that show on an empty query are scored by the same
    /// function, all at zero, and their order is therefore the order their
    /// supplier handed them over in.
    ///
    /// MUTATION: return `None` for an empty query — the palette opens empty and
    /// all three assertions go red.
    #[test]
    fn an_empty_query_matches_everything_and_highlights_nothing() {
        assert_eq!(
            fuzzy_score("", "New tab"),
            Some(FuzzyMatch {
                score: 0.0,
                hits: Vec::new()
            })
        );
        assert_eq!(
            fuzzy_score("", ""),
            Some(FuzzyMatch {
                score: 0.0,
                hits: Vec::new()
            })
        );
        assert_eq!(score("", "anything at all"), 0.0);
    }

    /// PIN — **a run beats the same characters scattered**.
    ///
    /// MUTATION: drop `RUN_WEIGHT`'s multiplication by `run` (use a flat
    /// weight) — the two spellings score the same and this goes red.
    #[test]
    fn adjacent_characters_beat_scattered_ones() {
        // Same three characters, same label, one contiguous and one spread.
        assert!(score("tab", "New tab") > score("nwt", "New tab"));
        assert!(score("set", "settings") > score("stg", "settings"));
    }

    /// PIN — **a word start is worth more than the middle of a word**, and the
    /// bonus is the *only* thing separating the pair that proves it.
    ///
    /// The first pair is the whole gate, and it is built rather than borrowed
    /// from real labels for a reason this test learned the hard way. It used to
    /// compare `gp` on `Git panel` against `gp` on `Toggle grep`, which reads
    /// well and is true — and is true **whether or not the bonus exists**,
    /// because those two are already separated by the gap term (3.55 against
    /// 2.65 with the bonus removed). A test that passes for a reason other than
    /// its name is a test that will go on passing when that reason is deleted.
    ///
    /// Here the query lands at the same offset in both strings, so the run is
    /// the same and the gap is the same, and the entire difference between them
    /// is whether the character before it was a break.
    ///
    /// MUTATIONS:
    /// (1) set `WORD_START_BONUS` to 0 — the two become equal and the first
    ///     assertion goes red;
    /// (2) test `hay[found]` instead of `hay[found - 1]` for the break — the
    ///     bonus is asked about `x` itself, neither string gets it, and the
    ///     first assertion goes red the same way.
    #[test]
    fn a_word_start_scores_above_the_middle_of_a_word() {
        assert!(
            score("x", "a x") > score("x", "aax"),
            "same run and same gap, so the space is the whole of the difference"
        );
        // `f32::EPSILON` is the gap between 1.0 and its neighbour, not slack for
        // a subtraction of numbers near five, so the tolerance is written as
        // what it is: far smaller than any term of the score, and far larger
        // than the last bit of an f32 at this magnitude.
        let difference = score("x", "a x") - score("x", "aax");
        assert!(
            (difference - super::WORD_START_BONUS).abs() < 1e-4,
            "the difference is exactly the bonus and not something resembling              it: {difference} against {}",
            super::WORD_START_BONUS
        );
        // The pair the section is actually about, kept because it is what the
        // rule is *for* — initials reaching a name — and asserted after the
        // pair that can fail on its own account.
        assert!(score("gp", "Git panel") > score("gp", "Toggle grep"));
        assert!(score("s", "Split") > score("s", "Close"));
    }

    /// PIN — **every one of the mock-up's break characters starts a word**, and
    /// whitespace means whitespace rather than the space alone.
    ///
    /// MUTATION: drop any character from `WORD_START_MARKS`, or narrow
    /// `is_break` to `ch == ' '` — the pair that character separates loses its
    /// bonus and its assertion goes red.
    #[test]
    fn each_break_character_starts_a_word() {
        for broken in [
            "New tab", "New-tab", "New_tab", "New:tab", "New·tab", "New\ttab",
        ] {
            assert!(
                score("nt", broken) > score("nt", "Newtab"),
                "{broken:?} should give `t` a word-start bonus"
            );
        }
    }

    /// PIN — **a long gap costs, but it stops costing past the cap**.
    ///
    /// Without the cap the score would be a statement about how long the label
    /// is; with it, two labels that both reach past the cap are separated by
    /// their runs and their word starts, which is what the reader can see.
    ///
    /// MUTATIONS:
    /// (1) remove the gap term — the first assertion goes red;
    /// (2) remove `.min(GAP_COST_CAP)` — the last two labels stop scoring the
    ///     same and the second assertion goes red.
    #[test]
    fn a_gap_costs_up_to_the_cap_and_no_further() {
        // Same single hit, one near and one far: the far one paid for the gap.
        assert!(score("z", "za") > score("z", "aaaaz"));
        // Both gaps are past the cap, so the two are indistinguishable.
        let nine = score("z", &format!("{}z", "a".repeat(9)));
        let twenty = score("z", &format!("{}z", "a".repeat(20)));
        assert!(
            (nine - twenty).abs() < f32::EPSILON,
            "gaps of 9 and 20 both cost the cap: {nine} vs {twenty}"
        );
    }

    /// PIN — **the case of the query does not change the answer**, in either
    /// direction and past ASCII.
    ///
    /// MUTATION: compare the characters without folding — the mixed-case
    /// assertions go red.
    #[test]
    fn matching_ignores_case() {
        assert_eq!(fuzzy_score("NT", "New tab"), fuzzy_score("nt", "New tab"));
        assert_eq!(fuzzy_score("nt", "NEW TAB"), fuzzy_score("NT", "new tab"));
        assert_eq!(hits("é", "Café"), vec![3]);
        assert_eq!(hits("É", "Café"), vec![3]);
    }

    /// PIN — **Chinese is matched by character**, and the hits address
    /// characters rather than bytes.
    ///
    /// Every character here is three bytes in UTF-8, so an implementation that
    /// counted bytes anywhere would return indices the painter would either
    /// panic on or highlight the wrong glyph with.
    ///
    /// MUTATIONS:
    /// (1) index the haystack by byte (`text.as_bytes()` / `find`) — the
    ///     indices come back as 3, 9 and this goes red;
    /// (2) fold with `to_ascii_lowercase` — unchanged here, which is why the
    ///     case test above carries `é` as well.
    #[test]
    fn chinese_is_matched_one_character_at_a_time() {
        assert_eq!(hits("设置", "设置"), vec![0, 1]);
        assert_eq!(hits("新签", "新建标签页"), vec![0, 3]);
        assert_eq!(fuzzy_score("签新", "新建标签页"), None);
        // Mixed scripts address one shared character axis.
        assert_eq!(hits("t设", "t 设置"), vec![0, 2]);
    }

    /// PIN — **the hits are the query's characters, in order, one per
    /// character**, and each one is where the painter should strike an accent.
    ///
    /// MUTATION: push `from` instead of `found` — the second assertion goes
    /// red, and the highlight would trail one character behind every match.
    #[test]
    fn the_hits_are_where_the_query_landed() {
        assert_eq!(hits("nt", "New tab"), vec![0, 4]);
        assert_eq!(hits("ta", "New tab"), vec![4, 5]);
        assert_eq!(hits("new", "New tab"), vec![0, 1, 2]);
        let long = hits("abc", "a__b__c");
        assert_eq!(long, vec![0, 3, 6]);
        // One hit per query character, always — a highlight that dropped one
        // would leave a matched character unpainted.
        assert_eq!(hits("aaa", "aaaa").len(), 3);
    }

    /// PIN — **the score is a pure function of its two arguments**, which is
    /// the property every ordering gate downstream leans on.
    ///
    /// MUTATION: carry `run` or `from` in a `static mut` / thread-local across
    /// calls — the repeat assertions go red.
    #[test]
    fn scoring_is_repeatable() {
        let once = fuzzy_score("nt", "New tab");
        let again = fuzzy_score("nt", "New tab");
        assert_eq!(once, again);
        // And no state leaks from a failed match into the next one.
        assert_eq!(fuzzy_score("zzz", "New tab"), None);
        assert_eq!(fuzzy_score("nt", "New tab"), once);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//   what the list is made of
// ═══════════════════════════════════════════════════════════════════════════

use std::path::PathBuf;

use bt_layout::SeatId;

use crate::TabId;
use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad};

use crate::i18n::Text;
use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::settings::{SettingsRow, push_float_window};
use crate::shortcuts::Action;

/// The five kinds of thing the palette can be aiming at, **in the order they
/// are always drawn**.
///
/// **They are sections and they never interleave** (DESIGN.md §7.55 ②). One
/// ranked list was the mock-up's shape and it was right there, because the
/// mock-up had two suppliers and thirty rows between them. This build has five,
/// and two of them are unbounded: a machine with a thousand shortcut rows and a
/// repository with thirty thousand files would, under one ranking, drown the
/// other three whenever a query happened to suit them — and the reader would
/// have no way to tell a section that had nothing to say from a section that
/// had been outscored. Fixed sections mean a query always answers all five
/// questions, and the answer to each is at a place on the glass the eye can
/// learn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    /// A verb, from the shortcut table.
    Actions,
    /// A place: one pane of this window.
    Places,
    /// A command that has already run, in any pane of this window.
    Commands,
    /// A file under a files column's root.
    Files,
    /// One row of the settings dialog.
    Settings,
}

impl Section {
    /// The order, and it is not renegotiated per query — see the type's note.
    ///
    /// Nearest first: a verb is about this window, a place is inside it, a
    /// command is inside a place, a file is on the disk under it, and a setting
    /// is about the whole product.
    pub const ORDER: [Self; 5] = [
        Self::Actions,
        Self::Places,
        Self::Commands,
        Self::Files,
        Self::Settings,
    ];

    /// The small muted line over the section.
    #[must_use]
    pub const fn heading(self) -> Text {
        match self {
            Self::Actions => Text::PaletteSectionActions,
            Self::Places => Text::PaletteSectionPlaces,
            Self::Commands => Text::PaletteSectionCommands,
            Self::Files => Text::PaletteSectionFiles,
            Self::Settings => Text::PaletteSectionSettings,
        }
    }

    /// How many rows of this section a query may put on the glass.
    ///
    /// Six for the two sections whose rows are one short phrase each and whose
    /// supply is a fixed table; eight for the three whose rows carry a path, a
    /// command line or a pane's name, because those are the sections a reader
    /// is scanning rather than reading, and a scan wants more of the list.
    #[must_use]
    pub const fn cap(self) -> usize {
        match self {
            Self::Actions | Self::Settings => SHORT_SECTION_CAP,
            Self::Places | Self::Commands | Self::Files => LONG_SECTION_CAP,
        }
    }

    /// Whether a section answers an **empty** query.
    ///
    /// The two that do are the two whose whole supply is small and about this
    /// window right now: every action this focus can reach, and every place in
    /// this window. The other three are answers to a question — which command,
    /// which file, which setting — and an empty box has not asked one; listing
    /// the first eight files of a repository in alphabetical order would be a
    /// list nobody wants and a walk nobody asked for.
    #[must_use]
    pub const fn answers_an_empty_query(self) -> bool {
        matches!(self, Self::Actions | Self::Places)
    }
}

/// DESIGN.md §7.55 ② — the cap on `Actions` and `Settings`.
const SHORT_SECTION_CAP: usize = 6;
/// DESIGN.md §7.55 ② — the cap on `Places`, `Commands` and `Files`.
const LONG_SECTION_CAP: usize = 8;

/// What pressing Enter on a row does.
///
/// Every arm names a verb this window already had — the palette is an index of
/// what this product can do and never a second implementation of any of it,
/// which is [`crate::Runtime::run_shortcut`]'s own rule applied at a second
/// door.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verb {
    /// Carry out one row of the shortcut table.
    Run(Action),
    /// Put the keyboard in one pane, activating its tab first.
    ///
    /// **The tab is named by its id and not by where it was sitting.** A
    /// position is not an address: the box stays up for as long as somebody is
    /// typing into it, and a shell that exits in that time takes its tab out of
    /// the list and moves every tab after it up one — an ordinal captured
    /// before that would name the wrong tab afterwards, and `activate_tab`
    /// would honour it. This is §2.4's "the address has to be unique all the
    /// way to the layer that reads it", read for a list that can shrink under a
    /// held answer.
    Go { tab: TabId, seat: SeatId },
    /// Scroll one pane to a command it already ran, activating its tab first.
    Recall {
        tab: TabId,
        seat: SeatId,
        mark: bt_term::CommandMarkId,
    },
    /// Open a file: onto a preview, and located in the column it came from.
    Open(PathBuf),
    /// Open the settings dialog on the page this row is on, scrolled to it.
    Adjust(SettingsRow),
}

/// One row a supplier is offering, before any query has been asked of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub section: Section,
    /// The row's main text — **the only string the query is matched against**.
    ///
    /// The hint is not searched, and that is a decision rather than an
    /// omission: a query that matched a path would light up rows whose printed
    /// name has none of the query's letters in it, and a reader cannot see why
    /// a row is in a list when the reason is written in text the highlight
    /// never touches.
    pub label: String,
    /// The muted text at the right end of the row.
    pub hint: Option<String>,
    /// The mark of the **thing** this row is about.
    ///
    /// `None` for the two sections whose rows are about an act rather than a
    /// thing: an action and a command have no object to draw, and this build
    /// has no artwork that means "a verb" — inventing one would be this column
    /// claiming a vocabulary the product does not have. The column is reserved
    /// in the measurement either way, so every row's text starts at the same
    /// place whether or not the row above it had a mark.
    pub mark: Option<ChromeMark>,
    /// Whether this place is holding a ticket — the fact the tab strip draws as
    /// a pulsing dot.
    pub awaiting: bool,
    pub verb: Verb,
}

/// One row, with the query's answer about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub what: Candidate,
    /// Which characters of [`Candidate::label`] the query landed on.
    pub hits: Vec<usize>,
}

/// One section's worth of the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub section: Section,
    pub rows: Vec<Row>,
    /// A single muted line drawn where the rows would be.
    ///
    /// The one note today is `Indexing…` under `Files`: a section that is going
    /// to have an answer shortly is a different thing from a section that has
    /// none, and a palette that showed nothing in both cases would teach a
    /// reader that this product cannot find their files.
    pub note: Option<Text>,
}

/// Everything a query returned, in section order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub blocks: Vec<Block>,
}

impl Listing {
    /// Every row of every block, in the order they are drawn — the axis the
    /// keyboard's selection is an index into.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.blocks.iter().flat_map(|block| block.rows.iter())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.blocks.iter().map(|block| block.rows.len()).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn row(&self, index: usize) -> Option<&Row> {
        self.rows().nth(index)
    }

    /// Whether there is nothing at all to draw — no rows and no note either,
    /// which is the state the empty line answers.
    #[must_use]
    pub fn says_nothing(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// Score `candidates` against `query` and deal them into sections.
///
/// `files_note` is the note the `Files` section carries when it has no rows to
/// show yet; it is passed in rather than decided here because whether an index
/// is still being built is a fact about a background lane, and this function is
/// pure.
///
/// A block with neither rows nor a note is dropped: a heading over nothing is a
/// promise the list cannot keep, which is the file menu's own rule about a
/// `Recent` heading over an empty list.
#[must_use]
pub fn arrange(candidates: &[Candidate], query: &str, files_note: Option<Text>) -> Listing {
    let query = query.trim();
    let empty_query = query.is_empty();
    let mut blocks = Vec::new();
    for section in Section::ORDER {
        // An empty box has asked nothing, so only the sections that answer
        // "what is here" answer it at all.
        if empty_query && !section.answers_an_empty_query() {
            continue;
        }
        let mut scored: Vec<(f32, Row)> = candidates
            .iter()
            .filter(|candidate| candidate.section == section)
            .filter_map(|candidate| {
                let found = fuzzy_score(query, &candidate.label)?;
                Some((
                    found.score,
                    Row {
                        what: candidate.clone(),
                        hits: found.hits,
                    },
                ))
            })
            .collect();
        // Stable, so rows the query cannot separate stay in the order their
        // supplier handed them over in — which for places is the tab order and
        // for commands is newest first, and in both cases is the order the
        // reader would have guessed.
        scored.sort_by(|left, right| right.0.total_cmp(&left.0));
        // The caps are what a *query* may put on the glass. An empty box is not
        // a query: it is the reader looking at what is here, and cutting that
        // to six would be this list deciding which of somebody's own panes are
        // worth showing.
        if !empty_query {
            scored.truncate(section.cap());
        }
        let rows: Vec<Row> = scored.into_iter().map(|(_, row)| row).collect();
        let note = if rows.is_empty() && section == Section::Files {
            files_note
        } else {
            None
        };
        if rows.is_empty() && note.is_none() {
            continue;
        }
        blocks.push(Block {
            section,
            rows,
            note,
        });
    }
    Listing { blocks }
}

/// Where the keyboard's selection goes next.
///
/// It wraps, which is the mock-up's `(palSel + d + len) % len` and its
/// argument: this is a list somebody is aiming down, and the row after the last
/// one is the first one — a walk that stopped dead at the bottom would make the
/// commonest gesture in the box (open it, press Up for the last row) impossible.
#[must_use]
pub fn step(len: usize, current: usize, forwards: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forwards {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//   the box
// ═══════════════════════════════════════════════════════════════════════════
//
// Every constant here is `design/ui-mockup.html` 2363-2394 read as a number,
// and every one of them is logical pixels — the layout multiplies by the
// surface's scale exactly once, at the top of `layout`.

/// `width: 540px` — the box's own width, before the window is consulted.
pub const PALETTE_WIDTH_LOGICAL_PX: f32 = 540.0;
/// `max-width: calc(100% - 48px)` — twenty-four a side.
pub const PALETTE_EDGE_MARGIN_LOGICAL_PX: f32 = 24.0;
/// `top: 84px`.
///
/// **A fixed offset from the top and not a fraction of the height**, which was
/// the other candidate. The reason for the position is the mock-up's own —
/// "top third so the eye's resting line isn't covered" — and that line is
/// measured from the top of the window, where the tab strip and the first rows
/// of every terminal are. A fraction would put the box halfway down a tall
/// monitor, over the very text the reader is aiming at.
pub const PALETTE_TOP_LOGICAL_PX: f32 = 84.0;
/// `border-radius: 10px`.
pub const PALETTE_RADIUS_LOGICAL_PX: f32 = 10.0;
/// `border: 1px solid var(--border)`.
pub const PALETTE_BORDER_LOGICAL_PX: f32 = 1.0;
/// `font-size: 13.5px` in the input.
pub const FIELD_FONT_LOGICAL_PX: f32 = 13.5;
/// The `14px` of the input's `padding: 12px 14px`.
pub const FIELD_PADDING_X_LOGICAL_PX: f32 = 14.0;
/// The `12px` of the same, doubled around an 18px line box, which is the
/// input's whole height.
pub const FIELD_HEIGHT_LOGICAL_PX: f32 = 42.0;
/// `border-bottom: 1px solid var(--border-soft)` under the input.
pub const FIELD_RULE_LOGICAL_PX: f32 = 1.0;
/// How far the caret stops short of the field's top and bottom, so a text
/// cursor reads as a cursor rather than as a rule.
pub const FIELD_CARET_INSET_LOGICAL_PX: f32 = 11.0;
/// `max-height: 336px` on `.pal-list`.
pub const LIST_MAX_HEIGHT_LOGICAL_PX: f32 = 336.0;
/// `padding: 5px` on `.pal-list`.
pub const LIST_PADDING_LOGICAL_PX: f32 = 5.0;
/// `.pal-item`'s `padding: 7px 10px` around a 12.5px line, rounded to an even
/// number of logical pixels so that no two rows in a column disagree by half a
/// pixel about where their middle is.
pub const ROW_HEIGHT_LOGICAL_PX: f32 = 30.0;
/// `.pal-item { border-radius: 7px }`.
pub const ROW_RADIUS_LOGICAL_PX: f32 = 7.0;
/// The `10px` of `.pal-item`'s padding.
pub const ROW_PADDING_X_LOGICAL_PX: f32 = 10.0;
/// `.pal-item { gap: 9px }` — between the mark's column and the text.
pub const ROW_GAP_LOGICAL_PX: f32 = 9.0;
/// `.pico { width: 16px }` — the column, reserved on every row.
pub const ROW_ICON_COLUMN_LOGICAL_PX: f32 = 16.0;
/// `.pico svg { width: 14px }` — the mark inside that column.
pub const ROW_ICON_LOGICAL_PX: f32 = 14.0;
/// `.pal-item { font-size: 12.5px }`.
pub const ROW_FONT_LOGICAL_PX: f32 = 12.5;
/// `.pal-hint { font-size: 11px }`.
pub const HINT_FONT_LOGICAL_PX: f32 = 11.0;
/// `.pal-hint { max-width: 45% }` — the hint never takes the row.
pub const HINT_MAX_FRACTION: f32 = 0.45;
/// The status dot's diameter, on the tab strip's own reading of the same fact.
pub const DOT_LOGICAL_PX: f32 = 6.0;
/// The air between the label and the dot after it.
pub const DOT_GAP_LOGICAL_PX: f32 = 7.0;
/// A section heading's own size — the hint's, because both are the muted voice.
pub const HEADING_FONT_LOGICAL_PX: f32 = 11.0;
/// The band a section heading is laid out in.
pub const HEADING_HEIGHT_LOGICAL_PX: f32 = 22.0;
/// `.pal-empty { padding: 14px; font-size: 12px }`.
pub const EMPTY_HEIGHT_LOGICAL_PX: f32 = 40.0;
/// The same rule's font size.
pub const EMPTY_FONT_LOGICAL_PX: f32 = 12.0;

/// One stretch of a row's label that is drawn in one colour and weight.
///
/// The runs are cut and **measured** at layout, not at paint, for the reason
/// the profile menu's accel column is: the painter must draw the very strings
/// that were measured, or the highlight walks away from the letters it is
/// supposed to be under.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedRun {
    pub text: String,
    pub rect: [f32; 4],
    /// Whether the query landed here — `mark { color: var(--accent);
    /// font-weight: 600 }`.
    pub matched: bool,
}

/// One row of the list, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedRow {
    /// Which row of the [`Listing`] this is, on the flat axis the keyboard
    /// walks. Paired with the rectangle so the paint and the hit test cannot
    /// disagree about which row is where.
    pub index: usize,
    /// The row's whole band — what a hover fills and a press lands in.
    pub rect: [f32; 4],
    /// The mark and the box it goes in, together — paired for the reason the
    /// index and the rectangle are: two lookups can disagree, one cannot.
    pub icon: Option<(ChromeMark, [f32; 4])>,
    pub runs: Vec<PlacedRun>,
    pub dot: Option<[f32; 4]>,
    pub hint: Option<([f32; 4], String)>,
}

/// A section heading, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedHeading {
    pub rect: [f32; 4],
    pub section: Section,
}

/// A section's note, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedNote {
    pub rect: [f32; 4],
    pub text: Text,
}

/// What the input is showing, as its owner assembled it.
///
/// Built by the caller rather than read out of the field here, for the reason
/// [`crate::Runtime::search_field_look`] exists: splicing the preedit into the
/// buffer for display is the owner's job, and the placeholder is a string this
/// module has no business choosing between.
#[derive(Clone, Copy, Debug)]
pub struct FieldLook<'a> {
    /// What is drawn in the box: the buffer with any preedit spliced in, or the
    /// placeholder when there is nothing typed.
    pub shown: &'a str,
    /// The text to the left of the caret, preedit included — measured to place
    /// the caret and nothing else.
    pub before: &'a str,
    /// Whether [`Self::shown`] is the reader's own typing rather than the
    /// placeholder.
    pub typed: bool,
}

/// Every rectangle the palette draws and hit-tests, in physical pixels of the
/// whole surface.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteLayout {
    scale: f32,
    frame: [f32; 4],
    /// The input's line box — the caret's own box, and the rectangle the IME's
    /// candidate window is asked to stand clear of.
    field: [f32; 4],
    /// The hairline under the input.
    rule: [f32; 4],
    /// The list's own box, which is also everything inside it is clipped to.
    list: [f32; 4],
    rows: Vec<PlacedRow>,
    headings: Vec<PlacedHeading>,
    notes: Vec<PlacedNote>,
    empty: Option<[f32; 4]>,
    /// How tall the list's contents are, scroll included — the number the
    /// clamp and [`Self::scroll_to_show`] are both derived from.
    content_height: f32,
    /// Where the caret sits, measured from the field's text origin.
    caret_x: f32,
    /// The string the field is showing — held here because the painter must
    /// draw the very text the caret was measured against.
    shown: String,
    /// Whether [`Self::shown`] is typing rather than the placeholder.
    typed: bool,
}

impl PaletteLayout {
    /// The caret's own bar, which is the **one** derivation the painter and the
    /// IME both read.
    ///
    /// Two derivations would be a candidate window that drifts away from the
    /// caret it is supposed to be under, which is exactly what the search
    /// capsule's [`crate::search::Capsule::caret_line`] exists to prevent.
    #[must_use]
    pub fn caret_line(&self) -> [f32; 4] {
        let inset = FIELD_CARET_INSET_LOGICAL_PX * self.scale;
        let left = self.field[0] + self.caret_x;
        [
            left,
            self.field[1] + inset,
            left + self.scale.max(1.0),
            self.field[3] - inset,
        ]
    }

    /// How far the list may be scrolled, at most.
    #[must_use]
    pub fn max_scroll(&self) -> f32 {
        (self.content_height - (self.list[3] - self.list[1])).max(0.0)
    }

    /// The least scroll that brings `index` wholly into the list, on
    /// `scrollIntoView({ block: "nearest" })`'s own terms.
    #[must_use]
    pub fn scroll_to_show(&self, index: usize, scroll: f32) -> f32 {
        let Some(row) = self.rows.iter().find(|placed| placed.index == index) else {
            return scroll;
        };
        // The row's rectangle already has the current scroll in it, so the
        // question is how far it is outside the box from where it stands.
        let above = self.list[1] - row.rect[1];
        let below = row.rect[3] - self.list[3];
        let moved = if above > 0.0 {
            scroll - above
        } else if below > 0.0 {
            scroll + below
        } else {
            scroll
        };
        moved.clamp(0.0, self.max_scroll())
    }
}

/// Where the box stands and what is in it.
///
/// `measure` is the chrome text measurer — the same one the menus take, so a
/// string this function reserves room for is a string the painter can draw at
/// the width it was promised.
#[must_use]
pub fn layout(
    surface: (f32, f32),
    scale: f32,
    listing: &Listing,
    look: FieldLook<'_>,
    scroll: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> PaletteLayout {
    let px = |logical: f32| logical * scale;
    let border = (PALETTE_BORDER_LOGICAL_PX * scale).max(1.0);
    let (surface_width, surface_height) = surface;

    let edge = px(PALETTE_EDGE_MARGIN_LOGICAL_PX);
    let width = px(PALETTE_WIDTH_LOGICAL_PX)
        .min(surface_width - 2.0 * edge)
        .max(0.0)
        .round();
    let left = ((surface_width - width) / 2.0).round();

    let field_height = px(FIELD_HEIGHT_LOGICAL_PX).round();
    let rule = (FIELD_RULE_LOGICAL_PX * scale).round().max(1.0);
    let list_padding = px(LIST_PADDING_LOGICAL_PX).round();
    let row_height = px(ROW_HEIGHT_LOGICAL_PX).round();
    let heading_height = px(HEADING_HEIGHT_LOGICAL_PX).round();
    let empty_height = px(EMPTY_HEIGHT_LOGICAL_PX).round();

    // How tall the contents want to be, before the box is capped.
    let mut content_height = 0.0f32;
    if listing.says_nothing() {
        content_height += empty_height;
    } else {
        for block in &listing.blocks {
            content_height += heading_height;
            #[expect(
                clippy::cast_precision_loss,
                reason = "a section is capped at LONG_SECTION_CAP rows, or at the number of \
                          panes in one window on an empty query"
            )]
            let rows = block.rows.len() as f32;
            content_height += rows * row_height;
            if block.note.is_some() {
                content_height += row_height;
            }
        }
    }
    let body_height = (content_height + 2.0 * list_padding)
        .min(px(LIST_MAX_HEIGHT_LOGICAL_PX))
        .round();
    let height = (2.0 * border + field_height + rule + body_height).round();

    // The box stands where the mock-up put it, and slides up only when it would
    // otherwise hang off the bottom of a short window.
    let top = px(PALETTE_TOP_LOGICAL_PX)
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];

    let field = [
        frame[0] + border,
        frame[1] + border,
        frame[2] - border,
        frame[1] + border + field_height,
    ];
    let rule_rect = [field[0], field[3], field[2], field[3] + rule];
    let list = [
        frame[0] + border,
        rule_rect[3],
        frame[2] - border,
        frame[3] - border,
    ];

    let text_origin = px(FIELD_PADDING_X_LOGICAL_PX);
    let caret_x = text_origin + measure(look.before, px(FIELD_FONT_LOGICAL_PX));

    let content_left = list[0] + list_padding;
    let content_right = list[2] - list_padding;
    let scroll = scroll.clamp(0.0, (content_height - (list[3] - list[1])).max(0.0));
    let mut cursor = list[1] + list_padding - scroll;

    let mut rows = Vec::new();
    let mut headings = Vec::new();
    let mut notes = Vec::new();
    let mut empty = None;

    if listing.says_nothing() {
        empty = Some([content_left, cursor, content_right, cursor + empty_height]);
    } else {
        let mut index = 0usize;
        for block in &listing.blocks {
            headings.push(PlacedHeading {
                rect: [
                    content_left + px(ROW_PADDING_X_LOGICAL_PX),
                    cursor,
                    content_right,
                    cursor + heading_height,
                ],
                section: block.section,
            });
            cursor += heading_height;
            for row in &block.rows {
                let band = [content_left, cursor, content_right, cursor + row_height];
                rows.push(place_row(band, index, row, scale, measure));
                cursor += row_height;
                index += 1;
            }
            if let Some(text) = block.note {
                notes.push(PlacedNote {
                    rect: [
                        content_left + px(ROW_PADDING_X_LOGICAL_PX),
                        cursor,
                        content_right,
                        cursor + row_height,
                    ],
                    text,
                });
                cursor += row_height;
            }
        }
    }

    PaletteLayout {
        scale,
        frame,
        field,
        rule: rule_rect,
        list,
        rows,
        headings,
        notes,
        empty,
        content_height,
        caret_x,
        shown: look.shown.to_owned(),
        typed: look.typed,
    }
}

/// Cut one row's label into runs and place everything on it.
fn place_row(
    band: [f32; 4],
    index: usize,
    row: &Row,
    scale: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> PlacedRow {
    let px = |logical: f32| logical * scale;
    let pad = px(ROW_PADDING_X_LOGICAL_PX);
    let icon_column = px(ROW_ICON_COLUMN_LOGICAL_PX);
    let icon_size = px(ROW_ICON_LOGICAL_PX);
    let middle = (band[1] + band[3]) / 2.0;

    let column_left = band[0] + pad;
    // Reserved whether or not this row has a mark — see `Candidate::mark`.
    let icon = row.what.mark.map(|mark| {
        let left = (column_left + (icon_column - icon_size) / 2.0).round();
        let top = (middle - icon_size / 2.0).round();
        (mark, [left, top, left + icon_size, top + icon_size])
    });

    let text_left = column_left + icon_column + px(ROW_GAP_LOGICAL_PX);
    let text_right = band[2] - pad;

    // The hint takes what it needs up to `max-width: 45%`, and the label gets
    // the rest — the mock-up's own division, which is what stops a long path
    // from pushing a pane's name off the row. Past that share it stops with an
    // ellipsis: `text-overflow: ellipsis` is written on this very rule
    // (mock-up 2389).
    let hint_font = px(HINT_FONT_LOGICAL_PX);
    let hint = row.what.hint.as_ref().map(|text| {
        let allowed = (band[2] - band[0]) * HINT_MAX_FRACTION;
        let text = crate::settings::ellipsized(text, allowed, hint_font, measure);
        let wide = measure(&text, hint_font);
        (
            [
                (text_right - wide).max(text_left),
                band[1],
                text_right,
                band[3],
            ],
            text,
        )
    });
    // **The dot's room is reserved before the label is laid**, on the mark
    // column's own reasoning: a box measured without it is a box the painter
    // then has to draw over something. And a pane whose name fills the row is
    // exactly the pane most likely to be the one that is waiting.
    let dot_size = px(DOT_LOGICAL_PX);
    let dot_room = if row.what.awaiting {
        dot_size + px(DOT_GAP_LOGICAL_PX)
    } else {
        0.0
    };
    let label_right = (hint
        .as_ref()
        .map_or(text_right, |(rect, _)| rect[0] - px(ROW_GAP_LOGICAL_PX))
        - dot_room)
        .max(text_left);

    // The runs are cut first and *then* fitted, so the accent still lands on
    // the letters that survive: a label ellipsized before it was cut would
    // carry hit indices addressing characters that are no longer drawn.
    let font = px(ROW_FONT_LOGICAL_PX);
    let mut runs = Vec::new();
    let mut at = text_left;
    for (text, matched) in cut_runs(&row.what.label, &row.hits) {
        if at >= label_right {
            break;
        }
        let room = label_right - at;
        let text = if measure(&text, font) <= room {
            text
        } else {
            crate::settings::ellipsized(&text, room, font, measure)
        };
        let wide = measure(&text, font);
        runs.push(PlacedRun {
            rect: [at, band[1], at + wide, band[3]],
            text,
            matched,
        });
        at += wide;
    }

    let dot = row.what.awaiting.then(|| {
        let left = (at + px(DOT_GAP_LOGICAL_PX)).clamp(text_left, label_right);
        let top = (middle - dot_size / 2.0).round();
        [left, top, left + dot_size, top + dot_size]
    });

    PlacedRow {
        index,
        rect: band,
        icon,
        runs,
        dot,
        hint,
    }
}

/// Cut `label` into alternating unmatched and matched runs.
///
/// `hits` are character offsets into `label`, ascending, each one appearing at
/// most once — which is exactly what [`fuzzy_score`] returns. Adjacent hits
/// coalesce into one run, so `nt` on `New tab` is two runs of one character and
/// `new` is one run of three: the highlight is a picture of where the query
/// landed, and three separate boxes around three touching letters would be a
/// picture of the loop that found them.
fn cut_runs(label: &str, hits: &[usize]) -> Vec<(String, bool)> {
    let mut runs: Vec<(String, bool)> = Vec::new();
    let mut hit = hits.iter().peekable();
    for (at, ch) in label.chars().enumerate() {
        let matched = if hit.peek() == Some(&&at) {
            hit.next();
            true
        } else {
            false
        };
        match runs.last_mut() {
            Some((text, was)) if *was == matched => text.push(ch),
            _ => runs.push((ch.to_string(), matched)),
        }
    }
    runs
}

/// What a point is over: a row, the box but no row, or nothing at all — the
/// three answers every menu in this window gives.
#[must_use]
pub fn hit(layout: &PaletteLayout, x: f64, y: f64) -> Option<Option<usize>> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a pointer position is a surface coordinate; the layout is in the same units"
    )]
    let (x, y) = (x as f32, y as f32);
    // **Only what is inside the list can be pressed**, and that is asked once.
    // A row scrolled half out of the box still has a rectangle reaching up
    // behind the input, and a press landing on that half must be the box rather
    // than the row — otherwise the input can be clicked *through* into whatever
    // happens to be under it. This used to be asked twice, here and again per
    // row, which is how it came to be untested: with the same question in two
    // places, deleting either one changed nothing.
    if contains(layout.list, x, y) {
        for row in &layout.rows {
            if contains(row.rect, x, y) {
                return Some(Some(row.index));
            }
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// The palette as one overlay layer.
///
/// One layer, so it carries one fade — [`crate::keyhint::build`]'s own reason.
#[must_use]
pub fn build(
    layout: &PaletteLayout,
    palette: &ChromePalette,
    selected: usize,
    opacity: f32,
) -> Vec<OverlayLayer> {
    let scale = layout.scale;
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let border = (PALETTE_BORDER_LOGICAL_PX * scale).max(1.0);

    let mut quads: Vec<OverlayQuad> = Vec::new();
    push_float_window(
        &mut quads,
        layout.frame,
        px(PALETTE_RADIUS_LOGICAL_PX),
        border,
        px(bt_render::FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.menu_shadow_inner_alpha),
        alpha(palette.menu_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );
    // The rule under the input, in the border's own ink.
    quads.push(OverlayQuad {
        rect: layout.rule,
        color: palette.menu_border,
        alpha: alpha(palette.menu_border_alpha),
    });

    let mut layer = OverlayLayer {
        quads,
        opacity,
        ..OverlayLayer::default()
    };

    // ── the input ──────────────────────────────────────────────────────────
    let field_text_rect = [
        layout.field[0] + px(FIELD_PADDING_X_LOGICAL_PX),
        layout.field[1],
        layout.field[2] - px(FIELD_PADDING_X_LOGICAL_PX),
        layout.field[3],
    ];
    layer.labels.push(ChromeLabel {
        mono: false,
        text: layout.shown.clone(),
        rect: field_text_rect,
        font_size_px: px(FIELD_FONT_LOGICAL_PX),
        color: if layout.typed {
            palette.menu_item_text
        } else {
            palette.menu_item_hint_text
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(layout.field),
    });
    layer.quads.push(OverlayQuad {
        rect: layout.caret_line(),
        color: palette.accent,
        alpha: 1.0,
    });

    // ── the list ───────────────────────────────────────────────────────────
    for heading in &layout.headings {
        if !overlaps(heading.rect, layout.list) {
            continue;
        }
        layer.labels.push(ChromeLabel {
            mono: false,
            text: heading.section.heading().text().to_owned(),
            rect: heading.rect,
            font_size_px: px(HEADING_FONT_LOGICAL_PX),
            color: palette.menu_item_hint_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Medium,
            tabular_numerals: false,
            clip: Some(layout.list),
        });
    }
    for note in &layout.notes {
        if !overlaps(note.rect, layout.list) {
            continue;
        }
        layer.labels.push(ChromeLabel {
            mono: false,
            text: note.text.text().to_owned(),
            rect: note.rect,
            font_size_px: px(ROW_FONT_LOGICAL_PX),
            color: palette.menu_item_hint_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(layout.list),
        });
    }
    for row in &layout.rows {
        if !overlaps(row.rect, layout.list) {
            continue;
        }
        // The pointer's row and the keyboard's row are the same state, because
        // in this box they are the same thing: the mock-up's pointer *selects*
        // on hover rather than lighting a second highlight beside the first.
        let on = selected == row.index;
        if on {
            // **Cropped to the list.** A label carries its own scissor and a
            // quad does not, so a row half scrolled past the fold would
            // otherwise paint its rounded ground straight over the input above
            // it. The crop squares off whichever end went past, which is what a
            // list scrolled to the middle of a row actually looks like.
            layer.quads.extend(
                bt_render::rounded_overlay_fill(
                    row.rect,
                    px(ROW_RADIUS_LOGICAL_PX),
                    palette.menu_item_hover,
                    1.0,
                )
                .into_iter()
                .filter_map(|quad| clipped(quad, layout.list)),
            );
        }
        let ink = if on {
            palette.menu_item_text_selected
        } else {
            palette.menu_item_text
        };
        // A sprite has no scissor either, and a mark cannot be cropped the way
        // a rectangle can — half a glyph is not half a picture of the thing. So
        // it is drawn when it is wholly inside the list and left out when it is
        // not, which is the files column's own rule for a row's icon.
        if let Some((mark, rect)) = row.icon
            && rect[1] >= layout.list[1]
            && rect[3] <= layout.list[3]
        {
            layer
                .sprites
                .push(ChromeSprite::new(mark, rect, palette.menu_item_hint_text));
        }
        for run in &row.runs {
            layer.labels.push(ChromeLabel {
                mono: false,
                text: run.text.clone(),
                rect: run.rect,
                font_size_px: px(ROW_FONT_LOGICAL_PX),
                // `mark { color: var(--accent); font-weight: 600 }` — the
                // matched letters keep the accent whether or not the row is the
                // selected one, because what they are saying is why this row is
                // in the list at all.
                color: if run.matched { palette.accent } else { ink },
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: if run.matched {
                    ChromeLabelWeight::SemiBold
                } else {
                    ChromeLabelWeight::Regular
                },
                tabular_numerals: false,
                clip: Some(layout.list),
            });
        }
        if let Some(rect) = row.dot {
            layer.quads.extend(
                bt_render::rounded_overlay_fill(
                    rect,
                    (rect[2] - rect[0]) / 2.0,
                    palette.accent,
                    1.0,
                )
                .into_iter()
                .filter_map(|quad| clipped(quad, layout.list)),
            );
        }
        if let Some((rect, text)) = &row.hint {
            layer.labels.push(ChromeLabel {
                mono: false,
                text: text.clone(),
                rect: *rect,
                font_size_px: px(HINT_FONT_LOGICAL_PX),
                color: palette.menu_item_hint_text,
                align_right: true,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: Some(layout.list),
            });
        }
    }
    if let Some(rect) = layout.empty {
        layer.labels.push(ChromeLabel {
            mono: false,
            text: Text::PaletteNoMatches.text().to_owned(),
            rect,
            font_size_px: px(EMPTY_FONT_LOGICAL_PX),
            color: palette.menu_item_hint_text,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(layout.list),
        });
    }
    vec![layer]
}

fn overlaps(rect: [f32; 4], clip: [f32; 4]) -> bool {
    rect[3] > clip[1] && rect[1] < clip[3]
}

/// The part of a quad inside `clip`, or nothing when the two do not meet —
/// [`crate::seats`]' own helper, for the reason it exists there: an overlay
/// quad has no scissor of its own, so a scrolling list has to do the cropping
/// itself.
fn clipped(quad: OverlayQuad, clip: [f32; 4]) -> Option<OverlayQuad> {
    let rect = [
        quad.rect[0].max(clip[0]),
        quad.rect[1].max(clip[1]),
        quad.rect[2].min(clip[2]),
        quad.rect[3].min(clip[3]),
    ];
    (rect[0] < rect[2] && rect[1] < rect[3]).then_some(OverlayQuad { rect, ..quad })
}

// ═══════════════════════════════════════════════════════════════════════════
//   the state
// ═══════════════════════════════════════════════════════════════════════════

/// The palette, while it is up.
///
/// App state and nothing else, on [`crate::profiles::ProfileMenu`]'s own
/// reasoning: it is not a seat, so the solver never sees it; it is not an
/// intent, so the session file never sees it. A palette that survived a restart
/// would be a window that opens mid-question.
#[derive(Clone, Debug)]
pub struct PaletteState {
    field: crate::text_field::TextField,
    /// What the current query returned, rebuilt whenever the query or a
    /// supplier changes.
    listing: Listing,
    selected: usize,
    scroll: f32,
    /// **The focus the window had at the moment the box opened**, which is what
    /// the `Actions` section is filtered against.
    ///
    /// It is captured rather than read live because opening the palette *is* a
    /// change of focus: with the box up the keyboard belongs to the box, so a
    /// live reading would say the terminal is not focused and would quietly
    /// drop every terminal-scoped row from the list. The honest question the
    /// section answers is "what could the hand that opened this have pressed",
    /// and that hand's focus is this one.
    opened_focus: crate::shortcuts::Focus,
}

impl PaletteState {
    /// A fresh box: nothing typed, the first row selected, scrolled to the top.
    #[must_use]
    pub fn opening(opened_focus: crate::shortcuts::Focus) -> Self {
        Self {
            field: crate::text_field::TextField::default(),
            listing: Listing::default(),
            selected: 0,
            scroll: 0.0,
            opened_focus,
        }
    }

    #[must_use]
    pub fn focus(&self) -> crate::shortcuts::Focus {
        self.opened_focus
    }

    #[must_use]
    pub fn field(&self) -> &crate::text_field::TextField {
        &self.field
    }

    pub fn field_mut(&mut self) -> &mut crate::text_field::TextField {
        &mut self.field
    }

    #[must_use]
    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }

    #[must_use]
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    pub fn set_scroll(&mut self, scroll: f32) {
        self.scroll = scroll;
    }

    /// The row Enter would run, if there is one.
    #[must_use]
    pub fn chosen(&self) -> Option<&Row> {
        self.listing.row(self.selected)
    }

    /// Put a freshly arranged answer in, and seat the selection.
    ///
    /// `requeried` says whether the *query* changed, and it decides where the
    /// selection lands: a new query starts at the top (the mock-up's `palSel =
    /// 0` on input), while a list that merely got longer because a background
    /// answer arrived keeps the row the reader was on — moving somebody's
    /// selection because a directory finished being walked would be this box
    /// taking the keyboard away mid-aim.
    pub fn refill(&mut self, listing: Listing, requeried: bool) {
        self.listing = listing;
        if requeried {
            self.selected = 0;
            self.scroll = 0.0;
        } else if self.listing.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.listing.len() - 1);
        }
    }

    /// Walk the selection one row.
    pub fn step(&mut self, forwards: bool) {
        self.selected = step(self.listing.len(), self.selected, forwards);
    }

    /// Put the selection on a row the pointer is over.
    ///
    /// **The pointer selects rather than lighting a second highlight**, which
    /// is the mock-up's `pointermove` handler and the reason the box has one
    /// notion of "the current row" instead of two: Enter must run what the
    /// reader is looking at, and a hand that has moved the pointer onto a row
    /// is looking at that row.
    ///
    /// Returns whether anything changed, so a caller can skip a repaint.
    pub fn point_at(&mut self, index: usize) -> bool {
        let changed = self.selected != index;
        self.selected = index;
        changed
    }
}

#[cfg(test)]
mod list_tests {
    use super::{
        Block, Candidate, LONG_SECTION_CAP, Listing, Row, SHORT_SECTION_CAP, Section, Verb,
        arrange, step,
    };
    use crate::i18n::Text;
    use crate::shortcuts::Action;

    /// A candidate of one section with one label, and a verb nobody runs.
    ///
    /// The verb is `Run(Action::NewTab)` for every section on purpose: these
    /// tests are about *arranging*, and a fixture whose verb varied with its
    /// section would let a bug that mixed up the two pass because the wrong
    /// thing still looked right.
    fn candidate(section: Section, label: &str) -> Candidate {
        Candidate {
            section,
            label: label.to_owned(),
            hint: None,
            mark: None,
            awaiting: false,
            verb: Verb::Run(Action::NewTab),
        }
    }

    fn many(section: Section, count: usize) -> Vec<Candidate> {
        (0..count)
            .map(|at| candidate(section, &format!("target {at}")))
            .collect()
    }

    fn sections(listing: &Listing) -> Vec<Section> {
        listing.blocks.iter().map(|block| block.section).collect()
    }

    fn labels(listing: &Listing) -> Vec<&str> {
        listing.rows().map(|row| row.what.label.as_str()).collect()
    }

    /// PIN — **the sections come out in one order, whatever the query is.**
    ///
    /// This is the ruling the whole shape rests on (DESIGN.md §7.55 ②): a query
    /// decides what is *in* a section and never where the section is, so a
    /// reader learns one arrangement instead of re-reading the box every time.
    ///
    /// MUTATIONS:
    /// (1) sort the blocks by their best score — the second case, whose only
    ///     good match is a setting, puts `Settings` first and this goes red;
    /// (2) reverse `Section::ORDER` — both cases go red.
    #[test]
    fn the_sections_are_always_in_the_same_order() {
        let mut all = Vec::new();
        for section in Section::ORDER {
            all.push(candidate(section, "zebra"));
        }
        assert_eq!(sections(&arrange(&all, "zebra", None)), Section::ORDER);

        // And with a query that suits the last section far better than the
        // first: `Settings` still comes last.
        let skewed = vec![
            candidate(Section::Actions, "zebra on our mat"),
            candidate(Section::Settings, "zoom"),
        ];
        assert_eq!(
            sections(&arrange(&skewed, "zoom", None)),
            vec![Section::Actions, Section::Settings]
        );
    }

    /// PIN — **rows of two sections are never interleaved.**
    ///
    /// The other half of the same ruling, and the half a single ranked list
    /// would break first: with one supplier holding thousands of rows, a
    /// scattered ordering would bury the other four.
    ///
    /// MUTATION: score every candidate into one list and sort it — the
    /// higher-scoring `Files` row lands between the two `Actions` rows and this
    /// goes red.
    #[test]
    fn two_sections_rows_never_interleave() {
        let mixed = vec![
            candidate(Section::Actions, "a zebra crossing"),
            candidate(Section::Files, "zebra"),
            candidate(Section::Actions, "another zebra"),
        ];
        let listing = arrange(&mixed, "zebra", None);
        assert_eq!(
            labels(&listing),
            vec!["a zebra crossing", "another zebra", "zebra"],
            "both actions, then the file — never the file between them"
        );
    }

    /// PIN — **each section takes at most its own cap.**
    ///
    /// MUTATIONS:
    /// (1) drop the `truncate` — every section returns twenty and this goes
    ///     red;
    /// (2) give every section one cap — the two halves disagree and one of
    ///     them goes red whichever number is chosen.
    #[test]
    fn a_query_takes_at_most_each_sections_cap() {
        let mut all = Vec::new();
        for section in Section::ORDER {
            all.extend(many(section, 20));
        }
        let listing = arrange(&all, "target", None);
        for block in &listing.blocks {
            assert_eq!(
                block.rows.len(),
                block.section.cap(),
                "{:?} is capped at its own number",
                block.section
            );
        }
        assert_eq!(
            listing.len(),
            2 * SHORT_SECTION_CAP + 3 * LONG_SECTION_CAP,
            "six, eight, eight, eight, six"
        );
    }

    /// PIN — **within a section the rows are in score order**, and ties keep
    /// the order their supplier handed them over in.
    ///
    /// The stability half is not decoration: `Places` are handed over in tab
    /// order and `Commands` newest-first, and an unstable sort would shuffle
    /// both into an order with no meaning at all whenever a query could not
    /// separate them.
    ///
    /// MUTATIONS:
    /// (1) sort ascending — the first assertion goes red;
    /// (2) use an unstable sort with a comparator that ignores the score —
    ///     the second assertion is where that shows, because every row there
    ///     scores the same.
    #[test]
    fn a_section_is_ordered_by_score_and_ties_keep_their_supplier_order() {
        let rows = vec![
            candidate(Section::Actions, "not a near match at all t"),
            candidate(Section::Actions, "tab"),
            candidate(Section::Actions, "a tab"),
        ];
        assert_eq!(
            labels(&arrange(&rows, "tab", None))[0],
            "tab",
            "the word itself beats the word inside a phrase"
        );

        let same = vec![
            candidate(Section::Places, "pane one"),
            candidate(Section::Places, "pane two"),
            candidate(Section::Places, "pane three"),
        ];
        assert_eq!(
            labels(&arrange(&same, "pane", None)),
            vec!["pane one", "pane two", "pane three"],
            "nothing separates them, so the supplier's order stands"
        );
    }

    /// PIN — **the empty box shows what is here, and only that.**
    ///
    /// Two halves, and each is a decision (DESIGN.md §7.55 ②). Only `Actions`
    /// and `Places` answer an empty query — the other three are answers to a
    /// question nobody has asked, and one of them would be a directory walk
    /// nobody asked for. And what they show is **everything**, uncapped: the
    /// cap is what a query may put on the glass, and an empty box is the reader
    /// looking at their own window rather than querying it.
    ///
    /// MUTATIONS:
    /// (1) let every section answer an empty query — the first assertion goes
    ///     red;
    /// (2) truncate on the empty path too — the second goes red;
    /// (3) return an empty listing for an empty query — both go red, and so
    ///     does the shape of the whole box.
    #[test]
    fn an_empty_query_shows_every_action_and_every_place_and_nothing_else() {
        let mut all = Vec::new();
        for section in Section::ORDER {
            all.extend(many(section, 20));
        }
        let listing = arrange(&all, "", None);
        assert_eq!(
            sections(&listing),
            vec![Section::Actions, Section::Places],
            "a box nobody has typed into has not asked about a command, a file \
             or a setting"
        );
        assert_eq!(
            listing.len(),
            40,
            "all twenty of each, because a cap is a query's rule"
        );
    }

    /// PIN — **a query that matches nothing anywhere returns nothing at all**,
    /// which is what puts `Nothing matches` on the glass.
    ///
    /// MUTATION: keep a block whose rows are empty — `says_nothing` turns
    /// false, the empty line is never drawn, and the box shows five headings
    /// over nothing.
    #[test]
    fn a_query_matching_nothing_says_nothing() {
        let mut all = Vec::new();
        for section in Section::ORDER {
            all.extend(many(section, 3));
        }
        let listing = arrange(&all, "qqqq", None);
        assert!(listing.says_nothing());
        assert!(listing.is_empty());
        assert_eq!(labels(&listing), Vec::<&str>::new());
    }

    /// PIN — **a section that is still being answered says so, and only when
    /// it has nothing to show.**
    ///
    /// The note is the difference between "there are no files called that" and
    /// "nobody has looked yet", and a box that drew the same picture for both
    /// would teach a reader that this product cannot find their files.
    ///
    /// MUTATIONS:
    /// (1) drop the note when the rows are empty — the first case loses its
    ///     block entirely and goes red;
    /// (2) attach the note whether or not there are rows — the second case
    ///     draws `Indexing…` under eight files and goes red;
    /// (3) attach the note to some other section — the third goes red.
    #[test]
    fn an_unanswered_files_section_carries_its_note() {
        let listing = arrange(&[], "anything", Some(Text::PaletteIndexing));
        assert_eq!(
            listing.blocks,
            vec![Block {
                section: Section::Files,
                rows: Vec::new(),
                note: Some(Text::PaletteIndexing),
            }],
            "a section with a note and no rows is still a section"
        );
        assert!(
            !listing.says_nothing(),
            "and the box does not also say `Nothing matches` over it"
        );

        let answered = many(Section::Files, 2);
        let listing = arrange(&answered, "target", Some(Text::PaletteIndexing));
        assert_eq!(listing.blocks[0].note, None, "rows outrank the note");

        // The note is the `Files` section's alone: an empty `Settings` section
        // is empty because the query missed, not because anybody is walking.
        let elsewhere = vec![candidate(Section::Settings, "theme")];
        let listing = arrange(&elsewhere, "zzz", Some(Text::PaletteIndexing));
        assert_eq!(sections(&listing), vec![Section::Files]);
    }

    /// PIN — **the flat index the keyboard walks addresses the rows in the
    /// order they are drawn**, across section boundaries.
    ///
    /// The selection is one number and the list is five blocks; if those two
    /// disagreed, Enter would run a row the reader is not looking at — the one
    /// failure in this box that silently does the wrong thing rather than
    /// nothing.
    ///
    /// MUTATION: have `Listing::rows` walk `blocks` in reverse — every
    /// assertion here goes red, and so does the third row's identity.
    #[test]
    fn the_flat_index_follows_the_drawn_order() {
        let all = vec![
            candidate(Section::Actions, "target one"),
            candidate(Section::Places, "target two"),
            candidate(Section::Settings, "target three"),
        ];
        let listing = arrange(&all, "target", None);
        assert_eq!(listing.len(), 3);
        assert_eq!(
            listing.row(0).map(|row| row.what.label.as_str()),
            Some("target one")
        );
        assert_eq!(
            listing.row(1).map(|row| row.what.label.as_str()),
            Some("target two")
        );
        assert_eq!(
            listing.row(2).map(|row| row.what.section),
            Some(Section::Settings)
        );
        assert_eq!(listing.row(3), None, "and there is no fourth");
    }

    /// PIN — **the walk wraps**, in both directions, and answers an empty list
    /// without arithmetic that would underflow.
    ///
    /// MUTATIONS:
    /// (1) clamp instead of wrapping — the two wrap assertions go red;
    /// (2) write the backwards step as `current - 1` — the wrap from 0 panics
    ///     on an unsigned subtraction, which is a red of the loudest kind.
    #[test]
    fn the_walk_wraps_at_both_ends() {
        assert_eq!(step(3, 0, true), 1);
        assert_eq!(step(3, 2, true), 0, "past the last row is the first");
        assert_eq!(step(3, 0, false), 2, "before the first row is the last");
        assert_eq!(step(3, 2, false), 1);
        assert_eq!(step(0, 0, true), 0, "an empty list has one answer");
        assert_eq!(step(0, 0, false), 0);
    }

    /// PIN — **the highlight is a picture of where the query landed**: touching
    /// hits are one run, and every character of the label survives the cut.
    ///
    /// MUTATIONS:
    /// (1) start a new run per hit instead of coalescing — the first assertion
    ///     goes red and the paint would draw three boxes around `new`;
    /// (2) skip a character when a hit is consumed — the round-trip assertion
    ///     goes red, and a letter would vanish off the row.
    #[test]
    fn the_runs_are_the_matched_and_unmatched_stretches() {
        use super::cut_runs;
        assert_eq!(
            cut_runs("New tab", &[0, 1, 2]),
            vec![("New".to_owned(), true), (" tab".to_owned(), false)]
        );
        assert_eq!(
            cut_runs("New tab", &[0, 4]),
            vec![
                ("N".to_owned(), true),
                ("ew ".to_owned(), false),
                ("t".to_owned(), true),
                ("ab".to_owned(), false),
            ]
        );
        assert_eq!(
            cut_runs("New tab", &[]),
            vec![("New tab".to_owned(), false)],
            "no query, one run, nothing accented"
        );
        // Whatever the hits are, the runs put the label back together.
        for hits in [
            vec![],
            vec![0],
            vec![6],
            vec![0, 3, 6],
            vec![0, 1, 2, 3, 4, 5, 6],
        ] {
            let rejoined: String = cut_runs("New tab", &hits)
                .into_iter()
                .map(|(text, _)| text)
                .collect();
            assert_eq!(rejoined, "New tab", "hits {hits:?} lost a character");
        }
        // And a Chinese label is cut on characters, not bytes.
        assert_eq!(
            cut_runs("命令面板", &[0, 1]),
            vec![("命令".to_owned(), true), ("面板".to_owned(), false)]
        );
    }

    /// PIN — **a row keeps its verb through the arrangement.**
    ///
    /// Scoring, sorting and capping all move rows about; the one thing that may
    /// never move is which verb is attached to which label, because that is the
    /// difference between Enter opening the pane the reader named and Enter
    /// opening a different one.
    ///
    /// MUTATION: build the `Row` from a neighbouring candidate (an off-by-one
    /// in the sort's rebuild) — this goes red.
    #[test]
    fn a_row_carries_the_verb_of_the_candidate_it_came_from() {
        let all = vec![
            Candidate {
                verb: Verb::Run(Action::OpenSettings),
                ..candidate(Section::Actions, "settings")
            },
            Candidate {
                verb: Verb::Run(Action::NewTab),
                ..candidate(Section::Actions, "set a new tab")
            },
        ];
        let listing = arrange(&all, "set", None);
        let found: Vec<(&str, &Verb)> = listing
            .rows()
            .map(|row| (row.what.label.as_str(), &row.what.verb))
            .collect();
        assert_eq!(
            found,
            vec![
                ("settings", &Verb::Run(Action::OpenSettings)),
                ("set a new tab", &Verb::Run(Action::NewTab)),
            ]
        );
    }

    /// PIN — **the query is trimmed, and a box holding only spaces is an empty
    /// box.**
    ///
    /// A reader who has typed a word and deleted it back to a space has not
    /// asked a question, and `fuzzy_score` would otherwise hunt for that space
    /// in every label — which most labels contain, so the answer would be a
    /// list ordered by where each row happens to put its first space.
    ///
    /// MUTATION: drop the `trim` — the space matches inside the labels, all
    /// five sections answer, and both assertions go red.
    #[test]
    fn a_query_of_spaces_is_no_query() {
        let mut all = Vec::new();
        for section in Section::ORDER {
            all.extend(many(section, 2));
        }
        let listing = arrange(&all, "   ", None);
        assert_eq!(sections(&listing), vec![Section::Actions, Section::Places]);
        assert_eq!(
            listing.rows().flat_map(|row| row.hits.clone()).count(),
            0,
            "and nothing is accented, because nothing was asked"
        );
    }

    /// PIN — **the selection survives a background answer and starts over on a
    /// new query.**
    ///
    /// The two halves are opposite and both are the same rule: the selection
    /// belongs to the reader. A keystroke is the reader changing the question,
    /// so the answer starts at the top; a directory finishing its walk is not,
    /// so the row under their eye stays under it.
    ///
    /// MUTATIONS:
    /// (1) reset on every refill — the second assertion goes red and a walk
    ///     finishing would move somebody's aim;
    /// (2) never reset — the first goes red and a new query would leave the
    ///     selection pointing into a list that no longer has that row.
    #[test]
    fn a_refill_keeps_the_readers_row_unless_the_query_changed() {
        use super::PaletteState;
        let five = many(Section::Places, 5);
        let mut state = PaletteState::opening(crate::shortcuts::Focus::default());
        state.refill(arrange(&five, "", None), true);
        state.step(true);
        state.step(true);
        assert_eq!(state.selected(), 2);

        state.refill(arrange(&five, "", None), false);
        assert_eq!(state.selected(), 2, "a supplier's answer is not the reader");

        state.refill(arrange(&five, "target", None), true);
        assert_eq!(state.selected(), 0, "a new question starts at the top");

        // And a list that shrank under a kept selection cannot leave it
        // pointing past the end.
        state.step(true);
        state.step(true);
        state.refill(arrange(&many(Section::Places, 2), "", None), false);
        assert!(state.selected() < 2, "clamped into the list that is there");
        state.refill(Listing::default(), false);
        assert_eq!(state.selected(), 0, "and an empty list selects nothing");
    }

    /// PIN — **a pre-edit is drawn and is not the query.**
    ///
    /// Typing `nihao` on a pinyin IME produces a running composition that is
    /// not any word yet; asking the list about it would make the box flicker
    /// through five answers to five non-words and land on the sixth. The field
    /// keeps the two apart and this is the assertion that it does.
    ///
    /// MUTATION: have `palette_ime` call `insert` for `Ime::Preedit` — the
    /// buffer stops being empty and the first assertion goes red. (The routing
    /// itself is pinned in `main.rs` by
    /// `the_palette_ime_keeps_a_preedit_out_of_the_query`.)
    #[test]
    fn a_composition_in_progress_is_not_in_the_query() {
        use super::PaletteState;
        let mut state = PaletteState::opening(crate::shortcuts::Focus::default());
        state.field_mut().set_preedit("ni'hao");
        assert_eq!(state.field().text(), "", "nothing is committed yet");
        assert_eq!(state.field().preedit(), "ni'hao", "and it is drawn");

        state.field_mut().insert("你好");
        assert_eq!(state.field().text(), "你好");
        assert_eq!(
            state.field().preedit(),
            "",
            "the commit takes the composition with it"
        );
    }

    /// PIN — **an empty query and a query that matches every row are not the
    /// same shape**, which is the thing an implementation that special-cased
    /// the empty query by "matching everything" would get wrong.
    ///
    /// MUTATION: treat an empty query as `fuzzy_score` returning `Some` for
    /// everything *and* let every section answer it — the `Files` block
    /// appears and this goes red.
    #[test]
    fn an_empty_query_is_not_a_query_that_matches_everything() {
        let all = vec![
            candidate(Section::Actions, "x"),
            candidate(Section::Files, "x"),
        ];
        assert_eq!(sections(&arrange(&all, "", None)), vec![Section::Actions]);
        assert_eq!(
            sections(&arrange(&all, "x", None)),
            vec![Section::Actions, Section::Files]
        );
    }

    /// A row for the geometry tests below.
    fn row(label: &str, hint: Option<&str>) -> Row {
        Row {
            what: Candidate {
                hint: hint.map(str::to_owned),
                ..candidate(Section::Actions, label)
            },
            hits: Vec::new(),
        }
    }

    fn listing_of(rows: Vec<Row>) -> Listing {
        Listing {
            blocks: vec![Block {
                section: Section::Actions,
                rows,
                note: None,
            }],
        }
    }

    /// A measure where every character is ten wide at any size, so widths are
    /// countable and a layout can be reasoned about on paper —
    /// [`crate::keyhint`]'s own fixture.
    fn ten_per_char(run: &str, _size: f32) -> f32 {
        run.chars().count() as f32 * 10.0
    }

    fn look<'a>(shown: &'a str, before: &'a str) -> super::FieldLook<'a> {
        super::FieldLook {
            shown,
            before,
            typed: true,
        }
    }

    /// What `crate::settings::ellipsized` puts on the end.
    const ELLIPSIS: char = '…';

    const SCALE: f32 = 1.0;
    const WINDOW: (f32, f32) = (1200.0, 800.0);

    /// PIN — **the box is 540 wide, centred, and stands where the mock-up put
    /// it** — until the window is too narrow for that, when it keeps its
    /// margins rather than its width.
    ///
    /// MUTATIONS:
    /// (1) drop the `min(surface_width - 2 * edge)` — the narrow window's box
    ///     hangs off both sides and the second assertion goes red;
    /// (2) place it at a fraction of the height — the first assertion goes
    ///     red.
    #[test]
    fn the_box_is_centred_in_the_upper_part_of_the_window() {
        let listing = listing_of(vec![row("New tab", None)]);
        let layout = super::layout(
            WINDOW,
            SCALE,
            &listing,
            look("", ""),
            0.0,
            &mut ten_per_char,
        );
        let frame = layout.frame;
        assert_eq!(frame[2] - frame[0], super::PALETTE_WIDTH_LOGICAL_PX);
        assert_eq!(
            (frame[0] + frame[2]) / 2.0,
            WINDOW.0 / 2.0,
            "centred across the window"
        );
        assert_eq!(frame[1], super::PALETTE_TOP_LOGICAL_PX);

        // A window narrower than the box keeps the mock-up's twenty-four a
        // side instead.
        let narrow = super::layout(
            (400.0, 800.0),
            SCALE,
            &listing,
            look("", ""),
            0.0,
            &mut ten_per_char,
        );
        assert_eq!(
            narrow.frame[0],
            super::PALETTE_EDGE_MARGIN_LOGICAL_PX,
            "and the margin is what survives"
        );
        assert_eq!(
            narrow.frame[2],
            400.0 - super::PALETTE_EDGE_MARGIN_LOGICAL_PX
        );
    }

    /// PIN — **the list stops growing at its own maximum and scrolls instead.**
    ///
    /// MUTATIONS:
    /// (1) drop the `min(LIST_MAX_HEIGHT_LOGICAL_PX)` — the tall box grows past
    ///     the cap and the second assertion goes red;
    /// (2) return `0.0` from `max_scroll` — the third goes red, and the rows
    ///     past the fold would be unreachable.
    #[test]
    fn a_long_list_stops_at_its_maximum_and_scrolls() {
        let short = listing_of((0..2).map(|at| row(&format!("row {at}"), None)).collect());
        let short = super::layout(WINDOW, SCALE, &short, look("", ""), 0.0, &mut ten_per_char);
        assert!(
            short.frame[3] - short.frame[1]
                < super::LIST_MAX_HEIGHT_LOGICAL_PX + super::FIELD_HEIGHT_LOGICAL_PX,
            "a short list makes a short box"
        );
        assert_eq!(short.max_scroll(), 0.0, "and nothing to scroll");

        let tall = listing_of((0..60).map(|at| row(&format!("row {at}"), None)).collect());
        let tall = super::layout(WINDOW, SCALE, &tall, look("", ""), 0.0, &mut ten_per_char);
        let list_height = tall.list[3] - tall.list[1];
        assert_eq!(
            list_height,
            super::LIST_MAX_HEIGHT_LOGICAL_PX,
            "sixty rows do not make a box sixty rows tall"
        );
        assert!(
            tall.max_scroll() > 0.0,
            "so the rest is reachable by scroll"
        );
    }

    /// PIN — **the scroll brings the selected row wholly into the box, and
    /// leaves it alone when it is already there.**
    ///
    /// `scrollIntoView({ block: "nearest" })`, which is the mock-up's own call
    /// and the only behaviour that does not yank the list about under a reader
    /// stepping through it one row at a time.
    ///
    /// MUTATIONS:
    /// (1) always centre the row — the "already visible" assertion goes red
    ///     and every arrow key would jump the list;
    /// (2) drop the clamp to `max_scroll` — the last assertion goes red.
    #[test]
    fn the_scroll_reaches_the_selected_row_and_no_further() {
        let listing = listing_of((0..60).map(|at| row(&format!("row {at}"), None)).collect());
        let layout = super::layout(
            WINDOW,
            SCALE,
            &listing,
            look("", ""),
            0.0,
            &mut ten_per_char,
        );
        assert_eq!(layout.scroll_to_show(0, 0.0), 0.0, "the first row is there");
        assert_eq!(layout.scroll_to_show(1, 0.0), 0.0, "and so is the second");
        let far = layout.scroll_to_show(59, 0.0);
        assert!(far > 0.0, "the last row is not");
        assert_eq!(far, layout.max_scroll(), "and reaching it reaches the end");
        assert_eq!(
            layout.scroll_to_show(999, 12.0),
            12.0,
            "a row that is not on the glass moves nothing"
        );
    }

    /// PIN — **a press lands on the row it looks like it lands on**, and the
    /// three answers a menu gives are the three this gives.
    ///
    /// MUTATIONS:
    /// (1) drop the `contains(layout.list, ..)` guard — a point over the input
    ///     answers with the first row and the second assertion goes red;
    /// (2) return `Some(Some(index))` for a point outside the frame — the last
    ///     assertion goes red and a click anywhere in the window would run a
    ///     verb.
    #[test]
    fn a_press_finds_the_row_under_it() {
        let listing = listing_of((0..4).map(|at| row(&format!("row {at}"), None)).collect());
        let layout = super::layout(
            WINDOW,
            SCALE,
            &listing,
            look("", ""),
            0.0,
            &mut ten_per_char,
        );
        let first = layout
            .rows
            .first()
            .expect("the list was laid out with rows in it");
        let middle = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };
        let (x, y) = middle(first.rect);
        assert_eq!(super::hit(&layout, x, y), Some(Some(0)));

        let (x, y) = middle(layout.field);
        assert_eq!(
            super::hit(&layout, x, y),
            Some(None),
            "the input is inside the box and is not a row"
        );

        assert_eq!(
            super::hit(&layout, 5.0, 5.0),
            None,
            "and the window behind it is not the box"
        );

        // **A row hanging over the top of the list is not pressable through the
        // input.** This is the case the whole guard exists for, and without it
        // nothing here can tell a `hit` that checks the list from one that does
        // not: with an unscrolled list every row is wholly inside it, so both
        // answer the same on every point.
        let tall = listing_of((0..60).map(|at| row(&format!("row {at}"), None)).collect());
        let scrolled = super::layout(
            WINDOW,
            SCALE,
            &tall,
            look("", ""),
            super::HEADING_HEIGHT_LOGICAL_PX + super::ROW_HEIGHT_LOGICAL_PX / 2.0,
            &mut ten_per_char,
        );
        let top = scrolled
            .rows
            .first()
            .expect("the list was laid out with rows in it");
        assert!(
            top.rect[1] < scrolled.list[1],
            "the fixture really does hang the first row over the fold"
        );
        let above = f64::from((top.rect[1] + scrolled.list[1]) / 2.0);
        let across = f64::from((top.rect[0] + top.rect[2]) / 2.0);
        assert_eq!(
            super::hit(&scrolled, across, above),
            Some(None),
            "the half of that row standing above the list belongs to the box"
        );
        let below = f64::from(scrolled.list[1] + 1.0);
        assert_eq!(
            super::hit(&scrolled, across, below),
            Some(Some(top.index)),
            "and the half inside it is still the row"
        );
    }

    /// PIN — **every row's text starts at the same place**, whether or not the
    /// row has a mark.
    ///
    /// The reserved column is the whole of what makes the list read as a column
    /// rather than as two ragged ones — the profile menu's `files_pane_accel`
    /// reasoning, which is that the frame is measured with the column in it, so
    /// the paint must place every row inside that measurement.
    ///
    /// MUTATION: start the text at `column_left` when there is no mark — the
    /// two rows disagree and this goes red.
    #[test]
    fn a_row_without_a_mark_still_leaves_room_for_one() {
        let with = Row {
            what: Candidate {
                mark: Some(crate::marks::ChromeMark::Gear),
                ..candidate(Section::Settings, "Theme")
            },
            hits: Vec::new(),
        };
        let listing = Listing {
            blocks: vec![Block {
                section: Section::Actions,
                rows: vec![row("New tab", None), with],
                note: None,
            }],
        };
        let layout = super::layout(
            WINDOW,
            SCALE,
            &listing,
            look("", ""),
            0.0,
            &mut ten_per_char,
        );
        let left = |at: usize| layout.rows[at].runs[0].rect[0];
        assert_eq!(left(0), left(1), "one column, whatever is in it");
        assert!(layout.rows[0].icon.is_none());
        assert!(layout.rows[1].icon.is_some());
    }

    /// PIN — **the caret is measured against the very text that is drawn**, and
    /// the IME reads the same rectangle the painter strikes.
    ///
    /// Two derivations here is a candidate list that drifts away from the
    /// letters it is offering to finish — the 2026-08-17 report, which is why
    /// [`super::PaletteLayout::caret_line`] is one function read twice.
    ///
    /// MUTATIONS:
    /// (1) measure the caret against `shown` instead of `before` — the caret
    ///     stands at the end of the line however far in the reader has walked,
    ///     and the second assertion goes red;
    /// (2) inset the caret by nothing — the third goes red.
    #[test]
    fn the_caret_stands_where_the_text_before_it_ends() {
        let listing = listing_of(vec![row("New tab", None)]);
        let at_end = super::layout(
            WINDOW,
            SCALE,
            &listing,
            look("abcd", "abcd"),
            0.0,
            &mut ten_per_char,
        );
        let midway = super::layout(
            WINDOW,
            SCALE,
            &listing,
            look("abcd", "ab"),
            0.0,
            &mut ten_per_char,
        );
        assert_eq!(
            at_end.caret_line()[0] - midway.caret_line()[0],
            20.0,
            "two characters of ten, and no more"
        );
        assert!(
            midway.caret_line()[0] < at_end.caret_line()[0],
            "a caret in the middle of the word is in the middle of the word"
        );
        let caret = at_end.caret_line();
        assert!(
            caret[1] > at_end.field[1] && caret[3] < at_end.field[3],
            "and it is a cursor rather than a rule down the whole field"
        );
    }

    /// PIN — **a label longer than the room it has stops with an ellipsis**,
    /// and never crosses into the hint or the waiting dot.
    ///
    /// A row is one line and the three things on it are measured against each
    /// other; a label drawn at its full width would run under the path beside
    /// it, because a chrome label is clipped to the *list* and not to its own
    /// box. §7.43's rule for this window's sentences — either wholly inside the
    /// box, or stopped with an ellipsis — read for a row instead of a card.
    ///
    /// MUTATIONS:
    /// (1) draw the runs at their measured width without fitting — the first
    ///     assertion goes red and the name runs under the path;
    /// (2) drop the dot's reserved room — the last assertions go red and the
    ///     dot lands on top of the last letters of the name.
    #[test]
    fn a_label_too_long_for_its_room_stops_with_an_ellipsis() {
        let long = "a-pane-with-a-very-long-name-that-cannot-possibly-fit-on-one-row";
        let plain = listing_of(vec![row(long, Some("C:/somewhere"))]);
        let layout = super::layout(WINDOW, SCALE, &plain, look("", ""), 0.0, &mut ten_per_char);
        let placed = &layout.rows[0];
        let (hint_rect, _) = placed.hint.as_ref().expect("the row has a hint");
        let right = placed.runs.last().map_or(0.0, |run| run.rect[2]);
        assert!(
            right <= hint_rect[0],
            "the name stops before the path begins: {right} vs {}",
            hint_rect[0]
        );
        let drawn: String = placed.runs.iter().map(|run| run.text.as_str()).collect();
        assert!(
            drawn.ends_with(ELLIPSIS) && drawn.chars().count() < long.chars().count(),
            "and it says it was cut: {drawn:?}"
        );

        // The same row, waiting: the dot has its own room and the name gives
        // way for it rather than being drawn under it.
        let waiting = Listing {
            blocks: vec![Block {
                section: Section::Places,
                rows: vec![Row {
                    what: Candidate {
                        awaiting: true,
                        hint: Some("C:/somewhere".to_owned()),
                        ..candidate(Section::Places, long)
                    },
                    hits: Vec::new(),
                }],
                note: None,
            }],
        };
        let layout = super::layout(
            WINDOW,
            SCALE,
            &waiting,
            look("", ""),
            0.0,
            &mut ten_per_char,
        );
        let placed = &layout.rows[0];
        let dot = placed.dot.expect("a waiting pane wears its dot");
        let right = placed.runs.last().map_or(0.0, |run| run.rect[2]);
        assert!(
            right <= dot[0],
            "the name stops before the dot: {right} vs {}",
            dot[0]
        );
        let (hint_rect, _) = placed.hint.as_ref().expect("and the path is still there");
        assert!(dot[2] <= hint_rect[0], "and the dot before the path");

        // **The reservation is what makes the label shorter**, and that is the
        // only thing that can prove it happened. The positions above cannot:
        // the dot is clamped to the label's right edge and the ellipsised label
        // ends exactly there, so they agree whether or not any room was set
        // aside. Two layouts of one row, differing only in whether it waits.
        let plain_end = super::layout(WINDOW, SCALE, &plain, look("", ""), 0.0, &mut ten_per_char)
            .rows[0]
            .runs
            .last()
            .map_or(0.0, |run| run.rect[2]);
        let waiting_end = placed.runs.last().map_or(0.0, |run| run.rect[2]);
        assert!(
            plain_end - waiting_end >= super::DOT_LOGICAL_PX,
            "a waiting row gives the dot its room out of the label's: {plain_end}              against {waiting_end}"
        );
    }

    /// PIN — **nothing the list draws escapes the list.**
    ///
    /// A [`ChromeLabel`] carries a scissor of its own and an [`OverlayQuad`]
    /// does not, which is easy to forget precisely because the labels look after
    /// themselves: the visible failure is a selected row's rounded ground
    /// painted straight up over the input box while the reader scrolls. A mark
    /// cannot be cropped at all — half a glyph is not half a picture of a thing
    /// — so it is drawn whole or not drawn.
    ///
    /// MUTATIONS:
    /// (1) push the selection's fill without `clipped` — the first assertion
    ///     goes red;
    /// (2) draw a mark that is only partly inside — the second goes red.
    #[test]
    fn nothing_the_list_draws_escapes_the_list() {
        let rows = (0..60)
            .map(|at| Row {
                what: Candidate {
                    mark: Some(crate::marks::ChromeMark::Gear),
                    ..candidate(Section::Settings, &format!("row {at}"))
                },
                hits: Vec::new(),
            })
            .collect();
        let listing = Listing {
            blocks: vec![Block {
                section: Section::Settings,
                rows,
                note: None,
            }],
        };
        // Past the heading and half into the first row, so the top row is cut
        // across its middle rather than merely pushed up under the heading.
        let scroll = super::HEADING_HEIGHT_LOGICAL_PX + super::ROW_HEIGHT_LOGICAL_PX / 2.0;
        let layout = super::layout(
            WINDOW,
            SCALE,
            &listing,
            look("", ""),
            scroll,
            &mut ten_per_char,
        );
        let colours = bt_render::chrome_palette();
        let painted = super::build(&layout, &colours, 0, 1.0);
        let layer = painted.first().expect("one layer, one fade");
        let list = layout.list;
        // The row the selection is on is the top one, and half of it is above
        // the fold — so its ground is the quad this is about. It is picked out
        // by its ink rather than by its geometry, because the box's own frame
        // legitimately spans the whole height and a geometric filter would have
        // to guess which quads were whose.
        let grounds: Vec<[f32; 4]> = layer
            .quads
            .iter()
            .filter(|quad| quad.color == colours.menu_item_hover)
            .map(|quad| quad.rect)
            .collect();
        assert!(
            !grounds.is_empty(),
            "the selected row is drawn with a ground at all"
        );
        for rect in &grounds {
            assert!(
                rect[1] >= list[1] - 0.5 && rect[3] <= list[3] + 0.5,
                "the selected row's ground at {rect:?} escaped {list:?}"
            );
        }
        assert!(
            layout.rows[0].rect[1] < list[1],
            "and the row it belongs to really was hanging over the fold"
        );
        for sprite in &layer.sprites {
            assert!(
                sprite.rect[1] >= list[1] && sprite.rect[3] <= list[3],
                "a mark at {:?} is not wholly inside {list:?}",
                sprite.rect
            );
        }
        assert!(
            !layer.sprites.is_empty(),
            "and the marks that do fit are still drawn"
        );
    }

    /// PIN — **the hint never takes more than the mock-up's 45% of the row**,
    /// and the label keeps the rest.
    ///
    /// MUTATION: let the hint take its measured width — the long path pushes
    /// the label's right edge to the left of its own start and this goes red.
    #[test]
    fn a_long_hint_does_not_take_the_row() {
        let listing = listing_of(vec![row(
            "main.rs",
            Some("a/very/long/relative/path/that/keeps/going/main.rs"),
        )]);
        let layout = super::layout(
            WINDOW,
            SCALE,
            &listing,
            look("", ""),
            0.0,
            &mut ten_per_char,
        );
        let placed = &layout.rows[0];
        let (hint_rect, _) = placed.hint.as_ref().expect("the row has a hint");
        let row_width = placed.rect[2] - placed.rect[0];
        assert!(
            hint_rect[2] - hint_rect[0] <= row_width * super::HINT_MAX_FRACTION + 0.5,
            "the hint is capped at its share of the row"
        );
        assert!(
            placed.runs[0].rect[0] < hint_rect[0],
            "and the label still starts to the left of it"
        );
    }
}
