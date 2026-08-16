//! The transcript's pattern engine: one place that answers "where does this pattern occur in this
//! text", for every consumer that will ever ask.
//!
//! Written now, with no user interface attached, on purpose. `docs/PROBLEM-LIST.md` P2-11 says the
//! same sentence three ways — in-pane search (§7.1.5d) looks for a pattern the user typed, the
//! y/n state detector (P1-2) looks for a question shape, and the automation brick (P3-4) waits for
//! a pattern to appear — and warns that implementing the match twice guarantees tearing one of
//! them out again later. So the matcher is a free function over `&str`, not a method on a search
//! bar.
//!
//! Three properties this module owes the rest of the tree:
//!
//! * **Linear time.** The `regex` crate is a finite-automaton engine with no backtracking, so
//!   `(a+)+b` — the pattern that turns a backtracking engine's 8 MB scan into a wall-clock
//!   minute — costs the same here as any other pattern. R1 in the search-block inventory names
//!   this as the mitigation to choose *before* shipping rather than after, and it is the reason
//!   `fancy-regex` (already in the tree behind syntect) is deliberately **not** what search runs
//!   on: backreferences buy nothing here and cost the guarantee.
//! * **Termination.** A pattern that can match the empty string (`a*`, `^`, `\b`) must not spin.
//!   See [`Matches`].
//! * **Counting the transcript, not the glyph soup.** The engine only ever sees one logical line's
//!   source text at a time, so the count cannot change with what the screen happens to be showing
//!   (user bug 2026-07-18: `m` counted 23 on a screen holding 8). That property is bought by the
//!   *caller* handing over `FrozenLine::text`; this module simply has no other input.

use crate::{StagingId, TranscriptId};

use regex::{Regex, RegexBuilder};

/// Which line a match was found on.
///
/// Two arms because the transcript owns two planes of text and search must span both: frozen
/// history, and the staged rows that have scrolled out of the grid but not yet been finalized into
/// a logical line (R7 — "the word is on my screen and search cannot find it" is the failure this
/// vocabulary exists to prevent).
///
/// [`crate::TranscriptStore::logical_lines`] only ever yields [`LineId::History`], because frozen
/// text is the only text this store owns as a `String`. Staged rows are still *cells*, and the
/// live grid is not in the transcript at all — so their text has to be handed in by the caller,
/// which is exactly why the id type has a second arm and why [`find_all`] takes an iterator rather
/// than reaching into the store itself.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LineId {
    History(TranscriptId),
    Staging(StagingId),
}

/// A half-open UTF-8 byte span inside one logical line's text.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

/// One hit: which line, and where in that line's source bytes.
///
/// Bytes rather than columns because the byte span is the fact and the column span is a rendering
/// of it; [`byte_range_to_columns`] does that conversion at paint time, against the same width
/// oracle the grid uses.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Match {
    pub line: LineId,
    pub range: ByteRange,
}

/// What the user typed into the capsule, before it becomes an engine.
///
/// The three booleans are the capsule's three toggles (`Aa` / `ab` / `.*`, DESIGN §7.1.5d) and all
/// three default to off.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// Why a query did not become an engine.
///
/// Two variants, not one, because the capsule shows three different things and needs to tell them
/// apart: an empty query is "nothing to search" and its count reads `""`, a broken pattern is
/// "this does not parse" and its count reads `"—"` with the typed text turned red in place, and a
/// compiled query reads `n/N`. Collapsing empty into a generic failure would put a red error on a
/// user who has simply not typed anything yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchError {
    /// The query is empty. Not an error the user made — a state the capsule sits in.
    Empty,
    /// The regex engine rejected the pattern. The payload is its own message, shown as-is.
    Pattern(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("empty search query"),
            Self::Pattern(message) => write!(formatter, "invalid search pattern: {message}"),
        }
    }
}

impl std::error::Error for SearchError {}

/// A query that has become an engine. Compile once per keystroke, scan many lines with it.
#[derive(Clone, Debug)]
pub struct CompiledSearch {
    regex: Regex,
}

impl CompiledSearch {
    /// The pattern actually handed to the engine, after escaping and word-boundary wrapping.
    /// Exposed for tests and diagnostics; nothing in the product should need to read it.
    pub fn pattern(&self) -> &str {
        self.regex.as_str()
    }
}

/// Turn a query into an engine.
///
/// Literal mode escapes the text, so a user searching for `C:\Users\` or `a.b` gets what they
/// typed and never a surprise metacharacter. Regex mode passes it through.
///
/// **Whole word wraps the whole pattern in `\b(?:…)\b`, and that `\b` cannot do CJK.** The reason
/// is not the implementation, it is the definition: a word boundary is a transition between a word
/// character and a non-word one, and Chinese, Japanese and Thai text has no such transitions to
/// find — telling words apart there needs real segmentation, which this engine does not do and
/// does not pretend to (inventory D-6).
///
/// One honest difference from the prototype worth recording, because it is visible: the mock ran
/// on JavaScript, whose `\b` is defined over ASCII word characters, so `\b中\b` *matched* there —
/// every ideograph read as its own word by accident. Rust's `regex` defines `\w` over Unicode, so
/// `中` is a word character and `\b中\b` finds nothing inside `中文`. Neither answer is
/// segmentation; the Unicode one is at least the one that is consistent with what `\w` means
/// everywhere else in the same pattern, so it is the one kept and pinned.
pub fn compile(query: &SearchQuery) -> Result<CompiledSearch, SearchError> {
    if query.text.is_empty() {
        return Err(SearchError::Empty);
    }
    let body = if query.regex {
        query.text.clone()
    } else {
        regex::escape(&query.text)
    };
    // The non-capturing group is load-bearing: `\bfoo|bar\b` binds the alternation looser than the
    // boundaries and would mean "`\bfoo` or `bar\b`", which is not whole-word anything.
    let pattern = if query.whole_word {
        format!(r"\b(?:{body})\b")
    } else {
        body
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!query.case_sensitive)
        .build()
        .map(|regex| CompiledSearch { regex })
        .map_err(|error| SearchError::Pattern(error.to_string()))
}

/// Every match in one logical line, left to right, non-overlapping.
///
/// **Non-overlapping is the regex engine's own semantics and is kept rather than worked around**:
/// `aa` in `aaa` is one match, not two, because the second `a` is consumed by the first match.
/// Every editor's find bar behaves this way, the count the user compares against is the count of
/// things they can press Enter through, and an overlapping scan would need a second cursor rule
/// that the `.*` toggle could not express anyway.
pub fn find_in_line<'a>(compiled: &'a CompiledSearch, line: &'a str) -> Matches<'a> {
    Matches {
        compiled,
        line,
        cursor: 0,
        done: false,
    }
}

/// The iterator behind [`find_in_line`], written by hand for one reason: zero-width protection.
///
/// `regex`'s own `find_iter` already advances past an empty match, so it terminates — but it also
/// *reports* those empty matches, and an empty match is not a hit: `a*` on a screen of prose would
/// paint a zero-width mark on every character boundary in the transcript and count them all. The
/// prototype hit this and fixed it the same way (`if (!m[0].length) { lastIndex++; continue }`,
/// mock 8552-8556, "a zero-width match must not spin"). Here the skip advances by one **character**
/// rather than one byte, because a byte cursor inside a multi-byte scalar is not a valid search
/// start and `find_at` would panic on it.
pub struct Matches<'a> {
    compiled: &'a CompiledSearch,
    line: &'a str,
    cursor: usize,
    done: bool,
}

impl Iterator for Matches<'_> {
    type Item = ByteRange;

    fn next(&mut self) -> Option<ByteRange> {
        while !self.done {
            let found = self.compiled.regex.find_at(self.line, self.cursor)?;
            if found.start() == found.end() {
                match self.line[found.end()..].chars().next() {
                    Some(character) => self.cursor = found.end() + character.len_utf8(),
                    // An empty match at the very end of the line: there is no next character to
                    // step onto, so the scan is finished rather than stuck.
                    None => self.done = true,
                }
                continue;
            }
            self.cursor = found.end();
            return Some(ByteRange {
                start: found.start(),
                end: found.end(),
            });
        }
        None
    }
}

/// Every match across a sequence of logical lines, in the order the lines are handed over.
///
/// Generic over nothing and allocating one `Vec`: the caller decides which plane's lines to feed
/// (history, staging, the live grid, or all three in document order) and this function neither
/// knows nor cares. That is what keeps it usable by P2-11's cross-session search and P3-4's
/// "wait until X appears" without either of them re-implementing the scan.
pub fn find_all<'a>(
    compiled: &CompiledSearch,
    lines: impl Iterator<Item = (LineId, &'a str)>,
) -> Vec<Match> {
    let mut matches = Vec::new();
    for (line, text) in lines {
        matches.extend(find_in_line(compiled, text).map(|range| Match { line, range }));
    }
    matches
}

/// Where a byte span lands on the terminal grid, as a half-open column span.
///
/// Measured through `bt-unicode`, Folio's single width oracle, so a hit's highlight covers exactly
/// the cells the same text occupies when the grid draws it: a CJK ideograph is two columns, a
/// combining mark is zero extra, an emoji ZWJ sequence is one cluster of two. Measuring with
/// `str::len` or `chars().count()` instead would put the mark half a character off on any line
/// holding a wide glyph.
///
/// An offset that lands inside a cluster rather than on its boundary widens outward to that
/// cluster's own columns — half a cluster does not have a column, and a highlight that stopped
/// mid-glyph would be painting a cell it does not own. Widths saturate at `u16::MAX`; a line long
/// enough to reach that has no addressable columns left anyway.
pub fn byte_range_to_columns(line: &str, range: ByteRange) -> (u16, u16) {
    let mut byte = 0usize;
    let mut column = 0usize;
    let mut start = None;
    let mut end = 0usize;
    for cluster in bt_unicode::graphemes(line) {
        let width = bt_unicode::cluster_width(cluster);
        if start.is_none() && range.start < byte + cluster.len() {
            start = Some(column);
        }
        if byte < range.end {
            end = column + width;
        }
        byte += cluster.len();
        column += width;
    }
    // A range beginning past the last cluster has no cluster to name, so it names the end of the
    // line — the same place the grid's own cursor would sit.
    let start = start.unwrap_or(column);
    (clamp_column(start), clamp_column(end.max(start)))
}

fn clamp_column(column: usize) -> u16 {
    u16::try_from(column).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapturedRow, TranscriptStore};
    use std::num::NonZeroUsize;
    use std::time::Instant;

    fn query(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.to_owned(),
            ..SearchQuery::default()
        }
    }

    fn ranges(pattern: &SearchQuery, line: &str) -> Vec<ByteRange> {
        find_in_line(&compile(pattern).unwrap(), line).collect()
    }

    #[test]
    fn a_literal_query_searches_for_what_was_typed_and_not_for_a_pattern() {
        let hits = ranges(&query("a.b"), "a.b axb aXb");
        assert_eq!(hits, vec![ByteRange { start: 0, end: 3 }]);
        let hits = ranges(&query(r"C:\Users\"), r"cd C:\Users\ ok");
        assert_eq!(hits, vec![ByteRange { start: 3, end: 12 }]);
    }

    #[test]
    fn case_folding_is_on_until_the_aa_toggle_turns_it_off() {
        assert_eq!(ranges(&query("err"), "Err err ERR").len(), 3);
        let sensitive = SearchQuery {
            case_sensitive: true,
            ..query("err")
        };
        assert_eq!(
            ranges(&sensitive, "Err err ERR"),
            vec![ByteRange { start: 4, end: 7 }]
        );
    }

    #[test]
    fn whole_word_refuses_a_hit_that_is_only_part_of_a_longer_word() {
        let whole = SearchQuery {
            whole_word: true,
            ..query("cat")
        };
        assert_eq!(
            ranges(&whole, "cat concat cats cat."),
            vec![
                ByteRange { start: 0, end: 3 },
                ByteRange { start: 16, end: 19 },
            ]
        );
    }

    #[test]
    fn whole_word_wraps_the_entire_pattern_so_an_alternation_still_means_whole_word() {
        let whole = SearchQuery {
            whole_word: true,
            regex: true,
            ..query("cat|dog")
        };
        assert_eq!(
            compile(&whole).unwrap().pattern(),
            r"\b(?:cat|dog)\b",
            "without the group the boundaries would bind to only one branch"
        );
        assert_eq!(ranges(&whole, "concat dog").len(), 1);
    }

    /// The CJK limit stated out loud, because it is a limit and not a bug (inventory D-6).
    ///
    /// `\b` asks for a transition between a word character and a non-word one. `中文` is two word
    /// characters in a row, so there is no transition to find, so whole-word search inside it
    /// finds nothing — and it would take real segmentation, not a better regex, to change that.
    /// The prototype's JavaScript `\b` answered differently only because its `\w` stops at ASCII.
    #[test]
    fn whole_word_finds_nothing_inside_cjk_because_b_is_not_segmentation() {
        let whole = SearchQuery {
            whole_word: true,
            ..query("中")
        };
        assert!(ranges(&whole, "中文").is_empty());
        assert_eq!(
            ranges(&whole, "a 中 b").len(),
            1,
            "spaces do make boundaries"
        );
        assert_eq!(
            ranges(&query("中"), "中文").len(),
            1,
            "plain search is fine"
        );
    }

    #[test]
    fn regex_mode_compiles_the_typed_pattern_and_a_broken_one_says_so() {
        let pattern = SearchQuery {
            regex: true,
            ..query(r"e\d+")
        };
        assert_eq!(
            ranges(&pattern, "e12 exit e3"),
            vec![
                ByteRange { start: 0, end: 3 },
                ByteRange { start: 9, end: 11 },
            ]
        );
        let broken = SearchQuery {
            regex: true,
            ..query("a(")
        };
        assert!(matches!(
            compile(&broken),
            Err(SearchError::Pattern(message)) if !message.is_empty()
        ));
    }

    #[test]
    fn an_empty_query_is_a_state_and_not_a_broken_pattern() {
        assert!(matches!(compile(&query("")), Err(SearchError::Empty)));
    }

    /// A backtracking engine is what makes `(a+)+b` a denial of service. This one is not.
    #[test]
    fn a_pathological_backtracking_pattern_is_ordinary_work_here() {
        let pattern = SearchQuery {
            regex: true,
            ..query("(a+)+b")
        };
        let line = "a".repeat(64);
        let started = Instant::now();
        assert!(ranges(&pattern, &line).is_empty());
        assert!(
            started.elapsed().as_secs() < 5,
            "the automaton engine has no exponential path to take"
        );
    }

    /// Zero-width safety, both halves: the scan terminates, and it reports no empty hits.
    #[test]
    fn a_pattern_that_can_match_nothing_neither_spins_nor_counts_nothing() {
        let pattern = SearchQuery {
            regex: true,
            ..query("a*")
        };
        assert_eq!(
            ranges(&pattern, "bab"),
            vec![ByteRange { start: 1, end: 2 }]
        );
        assert_eq!(ranges(&pattern, ""), Vec::new());
        assert_eq!(ranges(&pattern, "bbb"), Vec::new());

        let anchor = SearchQuery {
            regex: true,
            ..query("^")
        };
        assert_eq!(ranges(&anchor, "hello"), Vec::new());

        // A multi-byte line proves the skip steps by character: a byte cursor landing inside `中`
        // would panic `find_at` rather than return.
        assert_eq!(
            ranges(&pattern, "中文a"),
            vec![ByteRange { start: 6, end: 7 }]
        );
    }

    /// The user bug of 2026-07-18 restated as a pin: eight `m`s count eight.
    #[test]
    fn eight_ms_on_a_line_count_eight_and_not_the_mocks_twenty_three() {
        assert_eq!(ranges(&query("m"), "mmmmmmmm").len(), 8);
    }

    /// Overlap semantics pinned so a later reader knows it was decided and not left to chance.
    #[test]
    fn overlapping_occurrences_count_once_because_a_match_consumes_its_text() {
        assert_eq!(
            ranges(&query("aa"), "aaa"),
            vec![ByteRange { start: 0, end: 2 }]
        );
        assert_eq!(ranges(&query("aa"), "aaaa").len(), 2);
    }

    #[test]
    fn a_hit_on_a_wide_glyph_covers_two_cells_per_ideograph() {
        let line = "中文abc中";
        let hits = ranges(&query("abc"), line);
        assert_eq!(byte_range_to_columns(line, hits[0]), (4, 7));
        let hits = ranges(&query("文"), line);
        assert_eq!(byte_range_to_columns(line, hits[0]), (2, 4));
        let hits = ranges(&query("中文"), line);
        assert_eq!(byte_range_to_columns(line, hits[0]), (0, 4));
    }

    #[test]
    fn a_combining_mark_and_an_emoji_cluster_measure_as_the_grid_measures_them() {
        let line = "e\u{301}x\u{1F469}\u{200D}\u{1F4BB}y";
        let hits = ranges(&query("x"), line);
        assert_eq!(byte_range_to_columns(line, hits[0]), (1, 2));
        let hits = ranges(&query("y"), line);
        assert_eq!(byte_range_to_columns(line, hits[0]), (4, 5));
    }

    #[test]
    fn find_all_reports_the_line_each_hit_came_from() {
        let lines = [
            (LineId::History(TranscriptId(1)), "error one"),
            (LineId::History(TranscriptId(2)), "clean"),
            (LineId::Staging(StagingId(9)), "error two error three"),
        ];
        let compiled = compile(&query("error")).unwrap();
        let found = find_all(&compiled, lines.into_iter());
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].line, LineId::History(TranscriptId(1)));
        assert_eq!(found[1].line, LineId::Staging(StagingId(9)));
        assert_eq!(found[2].line, LineId::Staging(StagingId(9)));
        assert_eq!(found[1].range, ByteRange { start: 0, end: 5 });
        assert_eq!(found[2].range, ByteRange { start: 10, end: 15 });
    }

    #[test]
    fn the_store_hands_over_its_frozen_lines_as_logical_lines() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(64).unwrap());
        store.capture(CapturedRow::plain("find ", true));
        store.capture(CapturedRow::plain("path entry", false));
        store.capture(CapturedRow::plain("second line", false));
        let lines = store.logical_lines().collect::<Vec<_>>();
        assert_eq!(
            lines.iter().map(|(_, text)| *text).collect::<Vec<_>>(),
            vec!["find path entry", "second line"],
            "a soft-wrapped pair is one logical line, which is why search never misses across it"
        );
        let compiled = compile(&query("find path")).unwrap();
        let found = find_all(&compiled, store.logical_lines());
        assert_eq!(found.len(), 1, "the wrap boundary is not a text boundary");
        assert_eq!(found[0].line, lines[0].0);
    }

    /// R1's number, measured rather than argued. No assertion beyond completing: the point is the
    /// wall-clock reading, which the ticket asks to be reported.
    #[test]
    fn a_hundred_thousand_lines_are_scanned_in_a_time_worth_printing() {
        let corpus = (0..100_000u32)
            .map(|index| {
                format!("2026-08-16 12:00:00 worker {index} finished task in {index} ms with cat")
            })
            .collect::<Vec<_>>();
        let lines = || {
            corpus
                .iter()
                .enumerate()
                .map(|(index, text)| (LineId::History(TranscriptId(index as u64)), text.as_str()))
        };

        let compiled = compile(&query("cat")).unwrap();
        let started = Instant::now();
        let found = find_all(&compiled, lines());
        let literal = started.elapsed();
        assert_eq!(found.len(), 100_000);
        println!(
            "R1: 100k logical lines ({} bytes), 3-char literal -> {} hits in {:?}",
            corpus.iter().map(String::len).sum::<usize>(),
            found.len(),
            literal
        );

        let zero_width = compile(&SearchQuery {
            regex: true,
            ..query("a*")
        })
        .unwrap();
        let started = Instant::now();
        let found = find_all(&zero_width, lines());
        println!(
            "R1: 100k logical lines, `a*` -> {} hits in {:?}",
            found.len(),
            started.elapsed()
        );
    }
}
