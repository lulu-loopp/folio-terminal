//! Folio's single cell-width oracle.

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};
use unicode_width::UnicodeWidthStr;

/// East Asian Ambiguous width policy.
///
/// M1 deliberately ships `Narrow`. P2-7 can expose this enum in user configuration without
/// introducing a second width implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmbiguousWidth {
    Narrow,
    Wide,
}

pub const DEFAULT_AMBIGUOUS_WIDTH: AmbiguousWidth = AmbiguousWidth::Narrow;

/// How many code points one extended grapheme cluster may hold.
///
/// A real cluster is tens of code points at the very most. Unicode itself says where the line is:
/// UAX #15's Stream-Safe Text Format (§13) caps a sequence at **30 non-starters** in a row and
/// inserts U+034F to break anything longer, which is the standard's own statement of "past here
/// it is not a cluster any more". Thirty-two covers that, and covers every sequence a program
/// actually sends — the longest RGI emoji ZWJ sequence, a four-person family with skin tones, is
/// eleven code points, and an emoji tag sequence for a subdivision flag is eight.
///
/// The ceiling matters because a cluster is re-measured and re-copied on every mark added to it,
/// on the thread that is drawing the window, and because the cell that holds it is copied into
/// every captured row. Ten thousand combining marks on one cell is not text; it is a child
/// choosing how much work the window does.
pub const MAX_GRAPHEME_CLUSTER_CHARS: usize = 32;

/// Measure one extended grapheme cluster and clamp it to the terminal consensus maximum of two
/// cells. `unicode-width` 0.2 owns emoji presentation/VS15/VS16 handling; this function owns the
/// product's ambiguous-width policy and the terminal-specific two-cell clamp.
pub fn cluster_width(cluster: &str) -> usize {
    cluster_width_with_ambiguous(cluster, DEFAULT_AMBIGUOUS_WIDTH)
}

pub fn cluster_width_with_ambiguous(cluster: &str, ambiguous: AmbiguousWidth) -> usize {
    let width = match ambiguous {
        AmbiguousWidth::Narrow => UnicodeWidthStr::width(cluster),
        AmbiguousWidth::Wide => UnicodeWidthStr::width_cjk(cluster),
    };
    width.min(2)
}

/// Measure text by UAX #29 extended grapheme clusters through the same oracle used by the grid.
pub fn text_width(text: &str) -> usize {
    graphemes(text).map(cluster_width).sum()
}

pub fn graphemes(text: &str) -> impl Iterator<Item = &str> {
    UnicodeSegmentation::graphemes(text, true)
}

/// Whether `next` extends the current UAX #29 extended grapheme cluster.
///
/// The terminal retains only the current cluster, so the segmentation library sees exactly the
/// context needed for GB11 (emoji ZWJ) and GB12/13 (regional-indicator parity) without retaining
/// unrelated terminal text.
pub fn extends_grapheme_cluster(current: &str, next: char) -> bool {
    if current.is_empty() {
        return false;
    }
    let mut candidate = String::with_capacity(current.len() + next.len_utf8());
    candidate.push_str(current);
    candidate.push(next);
    let cursor = current.len();
    !GraphemeCursor::new(cursor, candidate.len(), true)
        .is_boundary(&candidate, 0)
        .expect("the complete current cluster supplies all UAX #29 context")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_variation_and_clusters_follow_one_clamped_oracle() {
        for (text, expected) in [
            ("👨‍👩‍👧‍👦", 2),
            ("👍🏽", 2),
            ("e\u{301}", 1),
            ("☂\u{fe0e}", 1),
            ("☂\u{fe0f}", 2),
            ("⌚\u{fe0e}", 1),
            ("🇺🇸", 2),
        ] {
            assert_eq!(text_width(text), expected, "{text:?}");
            assert_eq!(cluster_width(text), expected, "{text:?}");
        }
    }

    #[test]
    fn ambiguous_defaults_narrow_and_has_one_future_configuration_seam() {
        assert_eq!(cluster_width("☆"), 1);
        assert_eq!(cluster_width_with_ambiguous("☆", AmbiguousWidth::Wide), 2);
        assert_eq!(text_width("A☆中│Ｂ"), 7);
    }

    #[test]
    fn uax29_extension_state_covers_combining_emoji_zwj_and_flags() {
        assert!(extends_grapheme_cluster("e", '\u{301}'));
        assert!(extends_grapheme_cluster("👍", '🏽'));
        assert!(extends_grapheme_cluster("👨\u{200d}", '👩'));
        assert!(extends_grapheme_cluster("🇺", '🇸'));
        assert!(!extends_grapheme_cluster("🇺🇸", '🇨'));
        assert!(!extends_grapheme_cluster("a", 'b'));
    }
}
