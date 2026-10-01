//! Soft-wrapped E6 controls for `docs/plans/design/pane-columns-2026-09-29.md` revision (f).
#![allow(clippy::disallowed_methods)]

use bt_detect::{
    DetectionContext, DetectionOptions, InlineMathSite, LiveCapture, LiveDetectionInput,
    LiveDetectionSource, PaneRect,
};

fn boundaries(text: &str) -> Vec<(u32, u32)> {
    let mut out = vec![(0, 0)];
    let (mut byte, mut cell) = (0u32, 0u32);
    for cluster in bt_unicode::graphemes(text) {
        byte += cluster.len() as u32;
        cell += bt_unicode::cluster_width(cluster) as u32;
        out.push((byte, cell));
    }
    out
}

fn capture(rows: &[String]) -> LiveCapture {
    let inputs: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(row, text)| {
            let continues = row == 10;
            let text = if continues {
                text.clone()
            } else {
                text.trim_end_matches([' ', '\t']).to_owned()
            };
            LiveDetectionInput {
                source: LiveDetectionSource::Grid {
                    row: row as u32,
                    revision: 1,
                },
                captured_columns: 101,
                cell_boundaries: boundaries(&text),
                text,
                continues,
                site: InlineMathSite::AltScreenContent,
            }
        })
        .collect();
    LiveCapture::new(
        inputs,
        DetectionContext::default(),
        DetectionOptions::default(),
    )
}

fn clipped_rule_screen(mut rows: impl FnMut(usize) -> (String, String)) -> Vec<String> {
    (0..40)
        .map(|row| {
            let (left, right) = rows(row);
            format!("{left:<50}\u{2502}{right}")
        })
        .collect()
}

fn rect(left: u32, right: u32) -> PaneRect {
    PaneRect {
        top: 0,
        bottom: 40,
        left,
        right,
    }
}

/// RED (69a round 3, E6) — **one soft-wrapped inline occurrence may have left/right/left
/// segments without any segment crossing the rule.** Its opening and closing delimiters are both
/// left of column 50, so E3(b) does not exempt it. E6 nevertheless permits the cut because each
/// mapped physical segment lies wholly on one side. A single min/max box for the occurrence spans
/// the rule and wrongly refuses the cut.
///
/// MUTATION: replace the per-segment test in `Measure::vertical_cut_crosses_a_proof` with one box
/// from the occurrence's minimum to maximum column.
#[test]
fn a_soft_wrapped_left_right_left_inline_occurrence_is_tested_per_segment() {
    let mut screen = clipped_rule_screen(|row| {
        if row == 11 {
            ("$c$".to_owned(), format!("right {row}"))
        } else {
            (format!("left {row}"), format!("right {row}"))
        }
    });
    screen[10] = format!("{:<50}\u{2502}{:<9}$b${:<38}", "$a$", "", "");
    assert_eq!(
        capture(&screen).frame().panes(),
        &[rect(0, 50), rect(51, 101)],
        "left/right/left segments leave the proven rule untouched"
    );
}

/// RED (69a round 3, E3 delimiter granularity) — **a soft-wrapped inline segment crossing a
/// proven full-height rule is not exempt merely because both delimiters are left of it.** The
/// opening delimiter is at row 10 column 0, that continued physical segment reaches the edge and
/// crosses column 50, and the same logical formula closes at row 11 column 3. E6 therefore keeps
/// the screen whole.
///
/// MUTATION: decide the veto from delimiter sides alone and ignore the mapped segment crossing.
#[test]
fn a_soft_wrapped_inline_segment_crossing_a_full_height_rule_vetoes_the_cut() {
    let mut screen = clipped_rule_screen(|row| {
        if row == 11 {
            ("a+y$".to_owned(), format!("right {row}"))
        } else {
            (format!("left {row}"), format!("right {row}"))
        }
    });
    screen[10] = format!("$x+{}\u{2502}{}", "a".repeat(47), "b".repeat(50));
    let capture = capture(&screen);
    assert_eq!(capture.frame().panes(), &[rect(0, 101)]);
}
