//! **`preview_trace`, as the application drives it.** Tests whose first assertion is about
//! `preview_trace`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::cell_ink;

/// **One rebuild of a whole document, in three clocks** — the harness both
/// budget tests measure with, so that the two are measuring one thing.
///
/// **What is in the clock and what is not.** The parse is real — and since
/// ticket T7 that is the *mapped* parse, because
/// [`preview::parse_markdown_ranged`] is [`preview::parse_markdown_mapped`]
/// with its maps dropped and the window builds the maps on every parse. The
/// fence highlighting is real (syntect, the half the research expected to
/// dominate), and the layout arithmetic is real; the *shaper* is the stub
/// below, for this legacy arithmetic-only probe. So this is the cost of
/// everything a rebuild re-derives except the proportional shaping.
///
/// The three constructions above the clocks are outside all of them, which is
/// where they belong: a palette and an empty picture map are a test's setup
/// and not a document's cost.
fn rebuild_cost(
    content: &str,
    cache: &mut MarkdownIntrinsicCache,
) -> (
    usize,
    std::time::Duration,
    std::time::Duration,
    std::time::Duration,
) {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let math = DocumentMath::default();
    let pictures = DocumentPictures::default();
    let art = PageArt {
        math: &math,
        pictures: &pictures,
        theme: bt_render::Theme::Dark,
    };
    let pass = IntrinsicPass {
        metrics,
        math: &math,
        palette: &palette,
        scale_ppm: scale_ppm(1.0),
        math_generation: 0,
    };
    let mut width_of = |runs: &[bt_render::PreviewRun], _: f32, _: f32| {
        runs.iter()
            .map(|run| run.text.chars().count())
            .sum::<usize>() as f32
            * 8.0
    };
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };

    let clock = Instant::now();
    let (blocks, ranges) = preview::parse_markdown_ranged(content);
    let parse = clock.elapsed();
    let clock = Instant::now();
    let intrinsic = measure_markdown_intrinsics(
        &blocks,
        MarkdownSourceBytes {
            content,
            ranges: &ranges,
        },
        pass,
        cache,
        &mut width_of,
    );
    let intrinsics = clock.elapsed();
    let clock = Instant::now();
    let source = MarkdownCaretBlock::Mono(MarkdownSourceBlock {
        index: 0,
        range: ranges[0].clone(),
        text: preview_live::block_source(content, &ranges[0]).to_owned(),
        lines: preview_edit::display_lines(preview_live::block_source(content, &ranges[0])),
        font_size: 14.0,
        line_height: 20.0,
        advance: 8.0,
    });
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        1000.0,
        metrics,
        art,
        &mut shaper,
    );
    let laid = clock.elapsed();
    assert_eq!(layout.len(), blocks.len());
    (blocks.len(), parse, intrinsics, laid)
}

/// **A page written in Chinese costs what a page written in English costs**
/// (user report, 2026-09-10: opening `README.zh-CN.md` froze the window).
///
/// The report's own hypothesis was that the parse or the provenance mapping
/// walks a document by *byte* where it means *character*, or searches from
/// the start of a block for every piece — either of which is quadratic, and
/// Chinese triples the byte count of the same page. This is the measurement
/// that would say so: the same harness the English budget above uses, over
/// 64 KiB of this repository's Chinese front page and 64 KiB of a page
/// written in both scripts, against the same one-frame budget.
///
/// **The reported document is two documents now** (2026-09-14). The front
/// page was cut down to a summary and its feature sections moved to
/// `docs/features.zh-CN.md`, so the Chinese prose the report was about is
/// mostly in the second file; both are padded to 64 KiB and both are asked.
///
/// **Mixed text is here beside pure Chinese because it is not the same
/// document** to this parser. A run of ideographs never reaches the flanking
/// rule, the link scanner or the code-span scanner at all; `**中文**english`
/// and `` `代码`中文 `` put a marker hard against a three-byte character on
/// both sides, which is where a walk that steps by bytes and a walk that
/// steps by characters first disagree. See
/// [`preview::MIXED_SCRIPT_PAGE`].
///
/// **What it said** (2026-09-10, the machine the report came from, beside
/// the English line above on the same run): 64 KiB of Chinese / 37 897
/// characters / 293 blocks — parse 0.99 ms, intrinsics 4 µs, layout 0.12 ms,
/// **total 1.12 ms**; 64 KiB of both scripts / 42 382 characters / 561
/// blocks — parse 1.27 ms, intrinsics 12 µs, layout 0.12 ms, **total 1.41
/// ms**; against English's 64 KiB / 200 blocks at **1.48 ms**. So a page of
/// Chinese costs *less* than the same weight of English and not more — the
/// parser walks bytes and Chinese spends three of them on a character, so
/// the same 64 KiB is fewer words, fewer spans and fewer delimiter runs. The
/// report's hypothesis is disproved by this line, and the line is here so
/// that it stays disproved.
///
/// The budget is one frame, on the English test's own terms and for its
/// reason: what it catches is a change of *shape* — a walk that became
/// quadratic on multi-byte text — and not a percentage on whatever machine
/// happens to run it. The numbers are printed as well as asserted, because a
/// ratio against the English line above is the reading that matters and a
/// number nobody can read is not a measurement.
///
/// MUTATION: give [`preview::TextOrigin`]'s `run_at` a scan from the start of
/// the file rather than of its own runs, or count a paragraph's characters to
/// find a byte, and this goes red while the English one stays green.
#[test]
fn a_page_written_in_chinese_rebuilds_inside_the_frame_budget() {
    for (name, one) in [
        ("Chinese", preview::CHINESE_PAGE),
        ("Chinese, the long half", preview::CHINESE_FEATURE_PAGE),
        ("Chinese and English", preview::MIXED_SCRIPT_PAGE),
    ] {
        let mut document = String::new();
        while document.len() < 64 * 1024 {
            document.push_str(one);
            document.push_str("\n\n");
        }
        let mut cache = MarkdownIntrinsicCache::default();
        rebuild_cost(&document, &mut cache);
        let mut typed = document.clone();
        // The midpoint of a page written in Chinese may fall inside a
        // character; step back to the boundary before slicing.
        let at = (0..=typed.len() / 2)
            .rev()
            .find(|&i| typed.is_char_boundary(i))
            .unwrap_or(0);
        let at = typed[..at].rfind('\n').map_or(0, |line| line + 1);
        typed.insert(at, 'x');
        let (blocks, parse, intrinsics, laid) = rebuild_cost(&typed, &mut cache);
        let total = parse + intrinsics + laid;
        println!(
            "{name}: {} bytes, {} characters, {blocks} blocks — parse {:?}, \
                 intrinsics {:?}, layout {:?}, total {:?}",
            typed.len(),
            typed.chars().count(),
            parse,
            intrinsics,
            laid,
            total,
        );
        assert!(
            total < std::time::Duration::from_millis(16),
            "{name}: a keystroke in a 64 KiB document has to fit in a frame \
                 whatever script it is written in, and this one took {total:?} \
                 (parse {parse:?}, intrinsics {intrinsics:?}, layout {laid:?})",
        );
    }
}
