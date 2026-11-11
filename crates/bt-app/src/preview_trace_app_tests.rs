//! **`preview_trace`, as the application drives it.** Tests whose first assertion is about
//! `preview_trace`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::preview_typing::{EditWork, one_character_edit_work};

/// **One line of a paragraph written in both scripts**, word for word the
/// second paragraph of [`preview::MIXED_SCRIPT_PAGE`]: markers pressed against
/// ideographs on both sides, two code spans a line (the scanner's claims), and a
/// line that ends in a comma, so that lines of it laid one under another stay
/// one paragraph joined by spaces — twice as many runs in its join as lines.
const BOTH_SCRIPTS_LINE: &str =
    "**中文**english and english**中文**, `代码`中文 and 中文`code`, *斜体*汉字,\n";

/// `one`, `copies` times, each copy its own run of blocks (a blank line after it).
fn copies_of(one: &str, copies: usize) -> String {
    let mut page = String::with_capacity((one.len() + 2) * copies);
    for _ in 0..copies {
        page.push_str(one);
        page.push_str("\n\n");
    }
    page
}

/// How many copies of `one` make a page of at least `bytes`.
fn copies_reaching(one: &str, bytes: usize) -> usize {
    bytes.div_ceil(one.len() + 2)
}

/// **Where the keystroke goes in a page**: one character into the first
/// paragraph that starts at or past the page's middle — inside a block whose
/// kind one more letter cannot change, as [`one_character_edit_work`] asks.
fn into_the_first_paragraph_past_the_middle(page: &str) -> usize {
    let (blocks, ranges, _) = preview::parse_markdown_mapped(page);
    let index = blocks
        .iter()
        .zip(&ranges)
        .position(|(block, range)| {
            range.start >= page.len() / 2 && matches!(block, preview::MarkdownBlock::Paragraph(_))
        })
        .expect("every fixture has a paragraph past its middle");
    let start = ranges[index].start;
    start + page[start..].chars().next().map_or(0, char::len_utf8)
}

/// **Twice the text, twice the work**: both walks' counts of the keystroke
/// over the doubled text against the same counts over the text, for `what`.
///
/// A walk whose work is linear in the text doubles, and one that searches its
/// runs takes a logarithm more (a little over twice at these sizes); one that walks
/// from the start for every piece quadruples. Three times is the line between
/// the two, and no machine's speed is in it.
fn twice_the_text_is_twice_the_work(what: &str, once: &EditWork, twice: &EditWork) {
    for (walk, once, twice) in [
        ("inline steps", once.inline_steps, twice.inline_steps),
        ("origin runs visited", once.origin_runs, twice.origin_runs),
    ] {
        assert!(
            once > 0,
            "{what}: the keystroke's parse counted no {walk}, so this measures nothing"
        );
        assert!(
            twice < 3 * once,
            "{what}: twice the text took {twice} {walk} against {once} — linear work \
             doubles, and a walk from the start for every piece quadruples"
        );
    }
}

/// **A page written in Chinese costs work in proportion to its length**
/// (user report, 2026-09-10: opening `README.zh-CN.md` froze the window).
///
/// The report's own hypothesis was that the parse or the provenance mapping
/// walks a document by *byte* where it means *character*, or searches from the
/// start of a block for every piece — either of which is quadratic, and Chinese
/// triples the byte count of the same page. The measurement of 2026-09-10 (64 KiB
/// of Chinese, of the long Chinese half and of both scripts each rebuilt in
/// under 1.5 ms, against English's 1.48 ms) seemed to disprove it, but it laid
/// whole pages end to end, and that never makes a paragraph longer: the walk
/// from a block's start was there, in `TextOrigin::run_at`, until 2026-11-11,
/// when the second doubling below was written for it. Until then the test asked
/// with the wall clock — one sample of one rebuild against a 16 ms frame — and a loaded
/// machine descheduled it past the frame in three landing chains in one week
/// (0.4.10 plan row 78). The claim was never the frame: it is the *shape*, and a
/// shape is counted (the English twin moved the same way on 2026-09-13,
/// `a_one_character_edit_reuses_unmodified_document_work`).
///
/// **Two doublings, because a walk can be quadratic in two things.** The three
/// pages are each laid end to end to 64 KiB and then to twice as many copies:
/// a walk from the start of the *file* for every piece is caught there. One
/// paragraph of [`BOTH_SCRIPTS_LINE`] is then 64 KiB of lines and twice as many:
/// a walk from the start of the *block* for every piece is caught there, which
/// the pages alone cannot catch, because repeating a page leaves every one of
/// its paragraphs as long as it was. Each keystroke owes the reuse
/// [`one_character_edit_work`] asserts as well: no fence or table cell measured
/// again, one prose block wrapped again, one mapped parse.
///
/// **What it counts and what it does not.** The inline scanner's steps and its
/// claim lookups, the delimiter pass's steps (`inline steps`), and the runs
/// [`preview_provenance::TextOrigin`] looks at to compose each piece through its
/// block's join (`origin runs visited`) — the walks the report's hypothesis
/// names. A super-linear walk in a loop that keeps no count is not seen here;
/// the counts cost nothing outside the tests, because the module that keeps them
/// is `#[cfg(test)]` and a count outside a `#[cfg(test)]` line does not build.
///
/// MUTATION (one per claim): find the run in `TextOrigin::run_at` by a walk
/// from the first run, as it was until 2026-11-11, and the paragraph's
/// `origin runs visited` quadruple; start the scanner's claim lookup in
/// `preview::scan_line_marked` from the line's first claim at every byte, and
/// the paragraph's `inline steps` do; bypass the wrap cache in
/// `preview_wrap::Pass::measure` (the shared lookup and the previous pass's
/// map both: either alone still finds the block), and the keystroke's reuse
/// goes red in [`one_character_edit_work`].
#[test]
fn a_chinese_page_costs_work_in_proportion_to_its_length() {
    for (name, one) in [
        ("Chinese", preview::CHINESE_PAGE),
        ("Chinese, the long half", preview::CHINESE_FEATURE_PAGE),
        ("Chinese and English", preview::MIXED_SCRIPT_PAGE),
    ] {
        let copies = copies_reaching(one, 64 * 1024);
        let [once, twice] = [copies, 2 * copies].map(|copies| {
            let page = copies_of(one, copies);
            let at = into_the_first_paragraph_past_the_middle(&page);
            one_character_edit_work(page, at)
        });
        twice_the_text_is_twice_the_work(name, &once, &twice);
    }

    let lines = (64 * 1024usize).div_ceil(BOTH_SCRIPTS_LINE.len());
    let [once, twice] = [lines, 2 * lines].map(|lines| {
        let paragraph = BOTH_SCRIPTS_LINE.repeat(lines);
        // Inside the middle line, in front of its `english`: one paragraph
        // still, whatever the letter does to the emphasis around it.
        let at = BOTH_SCRIPTS_LINE.len() * (lines / 2)
            + BOTH_SCRIPTS_LINE
                .find("english")
                .expect("the line carries English");
        one_character_edit_work(paragraph, at)
    });
    twice_the_text_is_twice_the_work("one paragraph in both scripts", &once, &twice);
}
