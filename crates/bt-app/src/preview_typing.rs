//! Non-GUI production-path measurements. Run explicitly with --ignored --nocapture.
use crate::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    time::Duration,
};

thread_local! {
    static TIMING: Cell<bool> = const { Cell::new(false) };
    static WORK: RefCell<BTreeMap<&'static str, usize>> = const { RefCell::new(BTreeMap::new()) };
    static TIMES: RefCell<BTreeMap<&'static str, Duration>> = const { RefCell::new(BTreeMap::new()) };
}

pub(crate) fn count(name: &'static str, amount: usize) {
    WORK.with_borrow_mut(|w| *w.entry(name).or_default() += amount);
}
fn work(name: &'static str) -> usize {
    WORK.with_borrow(|w| w.get(name).copied().unwrap_or_default())
}

/// Mutation: restore a String clone for the snapshot used by caret/rebuild.
#[test]
fn typing_snapshots_share_the_content_allocation() {
    let buffer = buffer("ordinary prose\n\n".repeat(12000));
    let snapshot = buffer.content.clone().unwrap();
    assert_eq!(snapshot.as_ptr(), buffer.content.as_ref().unwrap().as_ptr());
}

/// Mutation: copy/diff the full body instead of recording the replacement.
#[test]
fn typing_undo_copies_only_the_replaced_bytes() {
    let mut buffer = buffer("ordinary prose\n\n".repeat(12000));
    let mut caret = preview_edit::EditCaret::default();
    caret.place(buffer.content.as_deref().unwrap(), 100, false);
    WORK.with_borrow_mut(BTreeMap::clear);
    buffer.edit_by_caret(&mut caret, |text, caret| {
        preview_edit::insert(text, caret, "x")
    });
    assert_eq!(work("edit copied bytes"), 0);
    assert!(work("undo compared bytes") <= 1);
    assert_eq!(buffer.undo_edit().unwrap().caret, 100);
    assert_eq!(
        buffer.content.as_deref().unwrap(),
        "ordinary prose\n\n".repeat(12000)
    );
}

/// Mutation: restore a widest-line scan across the complete document on edit.
#[test]
fn typing_width_work_is_confined_to_changed_lines() {
    let mut buffer = buffer("ordinary prose\n\n".repeat(12000));
    let mut caret = preview_edit::EditCaret::default();
    caret.place(buffer.content.as_deref().unwrap(), 100, false);
    WORK.with_borrow_mut(BTreeMap::clear);
    buffer.edit_by_caret(&mut caret, |text, caret| {
        preview_edit::insert(text, caret, "x")
    });
    assert!(
        work("width bytes") <= 32,
        "measured {} bytes",
        work("width bytes")
    );
    assert_eq!(buffer.max_columns, 15);
}

pub(crate) struct Timer(&'static str, Option<Instant>);
impl Timer {
    pub(crate) fn new(name: &'static str) -> Self {
        Self(name, TIMING.with(Cell::get).then(Instant::now))
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(start) = self.1 {
            TIMES.with_borrow_mut(|t| *t.entry(self.0).or_default() += start.elapsed());
        }
    }
}

fn buffer(text: String) -> preview::PreviewBuffer {
    let mut buffer = preview::PreviewBuffer::new(
        preview::PreviewSource::file("typing-fixture.md"),
        "typing-fixture.md".into(),
    );
    buffer.accept(preview::HeadOutcome::Read {
        text,
        truncated: false,
        mtime: None,
        content_says_text: true,
        encoding: preview::HeadEncoding::Utf8,
        lossy: false,
    });
    buffer
}

fn source_at(
    content: &str,
    blocks: &[preview::MarkdownBlock],
    ranges: &[std::ops::Range<usize>],
    at: usize,
) -> Option<Box<MarkdownCaretBlock>> {
    let index = preview_live::caret_seat(content, ranges, at).block()?;
    let heading = markdown_prose_face(&blocks[index])?;
    let text = preview_live::block_source(content, &ranges[index]).to_owned();
    let metrics = seats::preview_markdown_metrics(1.0);
    let (font_size, line_height) =
        heading.map_or((metrics.font_size, metrics.line_height), |level| {
            (
                metrics.heading_font(level),
                metrics.heading_line_height(level),
            )
        });
    Some(Box::new(MarkdownCaretBlock::Prose(MarkdownProseBlock {
        index,
        range: ranges[index].clone(),
        lines: prose_source_lines(&text),
        text,
        heading: heading.is_some(),
        font_size,
        line_height,
    })))
}

/// Real CPU shaping, parsing, highlighting, intrinsics and cached layout. No GPU present.
#[test]
#[ignore = "wall-clock diagnostic; reads docs/DESIGN.md at run time"]
fn md_typing_path_benchmark() {
    TIMING.with(|timing| timing.set(true));
    let real =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/DESIGN.md"))
            .unwrap();
    let synthetic: String = (0..12000).map(|i| format!("Paragraph {i}: **bold text** with `inline code` and enough ordinary words to wrap. {}\n\n", "prose ".repeat(32))).collect();
    let mut fonts = bt_render::preview_measure_font_system();
    for (name, text) in [("DESIGN", real), ("synthetic", synthetic)] {
        let bytes = text.len();
        TIMES.with_borrow_mut(BTreeMap::clear);
        let accepted = Instant::now();
        let mut buffer = buffer(text);
        let accepted = accepted.elapsed();
        let open_width = TIMES.with_borrow(|t| t.get("width/index").copied().unwrap_or_default());
        let mut doc = PreviewDocument::default();
        let mut cache = MarkdownIntrinsicCache::default();
        let wraps = preview_wrap::WindowCache::default();
        let mut caret = preview_edit::EditCaret::default();
        // Choose a prose block near the byte midpoint, then the following prose block.
        let (blocks, ranges, _) =
            preview::parse_markdown_mapped(buffer.content.as_deref().unwrap());
        let candidates: Vec<_> = blocks
            .iter()
            .zip(&ranges)
            .filter(|(b, r)| r.start >= bytes / 2 && markdown_prose_face(b).is_some())
            .map(|(_, r)| r.clone())
            .take(2)
            .collect();
        caret.place(
            buffer.content.as_deref().unwrap(),
            candidates[0].start,
            false,
        );
        println!(
            "\n{name}: {bytes} bytes, {} blocks; milliseconds (percentage of measured total)",
            blocks.len()
        );
        println!(
            "| action | edit copy | undo diff | width/index | caret copy | caret scan | rebuild copy | parse | intrinsic | layout | other | total | shaper calls | parsed bytes | wrapped blocks |"
        );
        println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
        for action in ["open", "within", "across", "insert", "delete"] {
            TIMES.with_borrow_mut(BTreeMap::clear);
            WORK.with_borrow_mut(BTreeMap::clear);
            if action == "open" {
                TIMES.with_borrow_mut(|t| {
                    t.insert("width/index", open_width);
                });
            }
            let total = Instant::now();
            if matches!(action, "within" | "across") {
                if action == "across" {
                    caret.place(
                        buffer.content.as_deref().unwrap(),
                        preview_edit::previous_boundary(
                            buffer.content.as_deref().unwrap(),
                            candidates[1].start,
                        ),
                        false,
                    );
                }
                let clock = Timer::new("caret copy");
                let content = buffer.content.as_deref().unwrap();
                drop(clock);
                preview_edit::move_caret_indexed(
                    content,
                    buffer.line_starts(),
                    &mut caret,
                    preview_edit::Motion::Right,
                    false,
                    30,
                );
                TIMES.with_borrow_mut(|t| {
                    let scan = t.remove("line scan").unwrap_or_default();
                    t.insert("caret scan", scan);
                });
            } else if action == "insert" {
                buffer.edit_by_caret(&mut caret, |text, caret| {
                    preview_edit::insert(text, caret, "x")
                });
            } else if action == "delete" {
                buffer.edit_by_caret(&mut caret, |content, caret| {
                    preview_edit::backspace(content, caret)
                });
            }
            let mut calls = 0;
            if action != "within" {
                let clock = Timer::new("layout");
                let mut pass = wraps.prepare(&doc, true, preview_wrap::Frame::new(1000.0, 1.0, 0));
                drop(clock);
                let (blocks, ranges, maps, intrinsic) = if action == "across" {
                    let PreviewDocument::Markdown {
                        blocks,
                        ranges,
                        maps,
                        intrinsic,
                        ..
                    } = std::mem::take(&mut doc)
                    else {
                        panic!()
                    };
                    (blocks, ranges, maps, Some(intrinsic))
                } else {
                    let clock = Timer::new("rebuild copy");
                    let content = buffer.content.clone().unwrap();
                    drop(clock);
                    let clock = Timer::new("parse");
                    let (blocks, ranges, maps) = preview::parse_markdown_mapped(&content);
                    drop(clock);
                    (blocks, ranges, maps, None)
                };
                let content = buffer.content.as_deref().unwrap();
                let source = source_at(content, &blocks, &ranges, caret.caret);
                let math = DocumentMath::default();
                let pictures = DocumentPictures::default();
                let palette = bt_render::chrome_palette();
                let intrinsic = intrinsic.unwrap_or_else(|| {
                    let _clock = Timer::new("intrinsic");
                    measure_markdown_intrinsics(
                        &blocks,
                        MarkdownSourceBytes {
                            content,
                            ranges: &ranges,
                        },
                        IntrinsicPass {
                            metrics: seats::preview_markdown_metrics(1.0),
                            math: &math,
                            palette: &palette,
                            scale_ppm: scale_ppm(1.0),
                            math_generation: 0,
                        },
                        &mut cache,
                        &mut |runs, font, line| {
                            calls += 1;
                            bt_render::measure_preview_paragraph_width(&mut fonts, runs, font, line)
                        },
                    )
                });
                let clock = Timer::new("layout");
                let layout = preview_wrap::lay_markdown_out_cached(
                    &blocks,
                    &intrinsic,
                    source.as_deref(),
                    PageArt {
                        math: &math,
                        pictures: &pictures,
                        theme: bt_render::Theme::Dark,
                    },
                    &mut pass,
                    &mut |runs, width, font, line| {
                        calls += 1;
                        bt_render::measure_preview_paragraph(&mut fonts, runs, width, font, line)
                    },
                );
                drop(clock);
                doc = PreviewDocument::Markdown {
                    blocks,
                    ranges,
                    maps,
                    source,
                    intrinsic,
                    layout,
                    math,
                    pictures,
                    wrap: pass.document(),
                };
            }
            let total = (total.elapsed()
                + if action == "open" {
                    accepted
                } else {
                    Duration::ZERO
                })
            .as_secs_f64()
                * 1000.0;
            print!("| {action} |");
            let mut sum = 0.0;
            for phase in [
                "edit copy",
                "undo diff",
                "width/index",
                "caret copy",
                "caret scan",
                "rebuild copy",
                "parse",
                "intrinsic",
                "layout",
            ] {
                let ms = TIMES
                    .with_borrow(|t| t.get(phase).copied().unwrap_or_default())
                    .as_secs_f64()
                    * 1000.0;
                sum += ms;
                print!(" {ms:.3} ({:.1}%) |", ms / total * 100.0);
            }
            println!(
                " {:.3} ({:.1}%) | {total:.3} | {calls} | {} | {} |",
                total - sum,
                (total - sum) / total * 100.0,
                work("parsed bytes"),
                work("wrapped blocks")
            );
        }
    }
}

/// Deterministic replacement for the former wall-clock frame-budget test.
/// Mutation: bypass the wrap/intrinsic caches or parse the body twice per edit.
pub(super) fn assert_one_character_edit_reuses_work() {
    let text: String = (0..1024)
        .map(|i| format!("Paragraph {i} **bold** with `code` and ordinary prose.\n\n"))
        .collect();
    let text = text + "```rust\nfn main() {}\n```\n\n| name | value |\n| --- | --- |\n| x | y |\n";
    let mut buffer = buffer(text);
    let mut cache = MarkdownIntrinsicCache::default();
    let wraps = preview_wrap::WindowCache::default();
    let mut doc = PreviewDocument::default();
    let at = buffer
        .content
        .as_deref()
        .unwrap()
        .find("Paragraph 512")
        .unwrap()
        + 2;
    let mut caret = preview_edit::EditCaret::default();
    caret.place(buffer.content.as_deref().unwrap(), at, false);
    for edit in [false, true] {
        if edit {
            buffer.edit_by_caret(&mut caret, |text, caret| {
                preview_edit::insert(text, caret, "x")
            });
        }
        let mut pass = wraps.prepare(&doc, true, preview_wrap::Frame::new(1000.0, 1.0, 0));
        let content = buffer.content.as_deref().unwrap();
        WORK.with_borrow_mut(BTreeMap::clear);
        let (blocks, ranges, maps) = preview::parse_markdown_mapped(content);
        let parsed_bytes = work("parsed bytes");
        let source = source_at(content, &blocks, &ranges, caret.caret);
        let math = DocumentMath::default();
        let pictures = DocumentPictures::default();
        let palette = bt_render::chrome_palette();
        let mut widths = 0;
        let intrinsic = measure_markdown_intrinsics(
            &blocks,
            MarkdownSourceBytes {
                content,
                ranges: &ranges,
            },
            IntrinsicPass {
                metrics: seats::preview_markdown_metrics(1.0),
                math: &math,
                palette: &palette,
                scale_ppm: scale_ppm(1.0),
                math_generation: 0,
            },
            &mut cache,
            &mut |runs, _, _| {
                widths += 1;
                runs.iter().map(|r| r.text.len() as f32 * 8.0).sum()
            },
        );
        let mut shaped = 0;
        let layout = preview_wrap::lay_markdown_out_cached(
            &blocks,
            &intrinsic,
            source.as_deref(),
            PageArt {
                math: &math,
                pictures: &pictures,
                theme: bt_render::Theme::Dark,
            },
            &mut pass,
            &mut |runs, width, _, line| {
                shaped += 1;
                let ink: f32 = runs.iter().map(|r| r.text.len() as f32 * 8.0).sum();
                (ink / width).ceil().max(1.0) * line
            },
        );
        assert_eq!(layout.len(), blocks.len());
        if edit {
            assert_eq!(
                widths, 0,
                "unchanged fences and table cells retain intrinsics"
            );
            assert_eq!(shaped, 1, "only the edited prose block is re-measured");
            assert_eq!(work("wrapped blocks"), 1);
            assert!(parsed_bytes <= content.len(), "at most one mapped parse");
            let expected = preview::parse_markdown_mapped(content);
            assert_eq!(blocks, expected.0);
            assert_eq!(ranges, expected.1);
            assert_eq!(maps, expected.2);
        } else {
            assert!(widths > 0);
            assert_eq!(shaped, 1024);
        }
        doc = PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            source,
            intrinsic,
            layout,
            math,
            pictures,
            wrap: pass.document(),
        };
    }
}

/// Mutation: rescan the document on movement, miss a split/join boundary, or
/// leave a removed maximum in the multiset. The oracle is the former full scan.
#[test]
fn typing_indexes_match_full_scans_through_edits_undo_and_redo() {
    let mut buffer = buffer("wide\t中文\r\nsmall\n🙂 e\u{301}\n\n".repeat(4));
    let mut caret = preview_edit::EditCaret::default();
    for step in 0..128 {
        let content = buffer.content.as_deref().unwrap();
        let boundaries: Vec<_> = content
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(content.len()))
            .collect();
        caret.place(content, boundaries[step * 7 % boundaries.len()], false);
        caret.anchor = preview_edit::normalize(content, boundaries[step * 11 % boundaries.len()]);
        let inserted = ["x", "\t", "界", "e\u{301}", "\r\n", "\n", "", "🙂"][step % 8];
        WORK.with_borrow_mut(BTreeMap::clear);
        buffer.edit_by_caret(&mut caret, |content, caret| {
            preview_edit::insert(content, caret, inserted)
        });
        assert_eq!(work("indexed bytes"), 0, "replacement uses the delta");
        for direction in 0..3 {
            if direction == 1 {
                let _ = buffer.undo_edit();
            }
            if direction == 2 {
                let _ = buffer.redo_edit();
            }
            let content = buffer.content.as_deref().unwrap();
            assert_eq!(buffer.line_starts(), preview_edit::line_starts(content));
            assert_eq!(buffer.max_columns, preview::widest_line_columns(content));
            for motion in [
                preview_edit::Motion::Left,
                preview_edit::Motion::Right,
                preview_edit::Motion::Up,
                preview_edit::Motion::Down,
                preview_edit::Motion::LineStart,
                preview_edit::Motion::LineEnd,
                preview_edit::Motion::PageUp,
                preview_edit::Motion::PageDown,
            ] {
                let mut actual = caret;
                let mut expected = caret;
                WORK.with_borrow_mut(BTreeMap::clear);
                preview_edit::move_caret_indexed(
                    content,
                    buffer.line_starts(),
                    &mut actual,
                    motion,
                    true,
                    3,
                );
                assert_eq!(work("indexed bytes"), 0, "movement borrows the index");
                preview_edit::move_caret(content, &mut expected, motion, true, 3);
                assert_eq!(actual, expected);
            }
        }
    }
}
