use super::*;

pub(super) struct Cpu {
    fonts: bt_render::PreviewMeasureFontSystem,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            fonts: bt_render::preview_measure_font_system(),
        }
    }
}

impl Measure for Cpu {
    fn width(&mut self, runs: &[bt_render::PreviewRun], font: f32, line: f32) -> f32 {
        preview_typing::count("shaper calls", 1);
        bt_render::measure_preview_paragraph_width(&mut self.fonts, runs, font, line)
    }
    fn wrap(&mut self, runs: &[bt_render::PreviewRun], width: f32, font: f32, line: f32) -> f32 {
        preview_typing::count("shaper calls", 1);
        bt_render::measure_preview_paragraph(&mut self.fonts, runs, width, font, line)
    }
    fn rows(&mut self, paragraph: &bt_render::PreviewParagraph) -> Vec<bt_render::PreviewTextRow> {
        preview_typing::count("shaper calls", 1);
        bt_render::measure_preview_text_rows(&mut self.fonts, paragraph)
    }
}

pub(crate) struct Harness {
    pub doc: PreviewDocument,
    pub text: String,
    pub view: View,
    pub width: f32,
    pub scale: f32,
    pub offsets: Vec<f32>,
    cache: preview_wrap::WindowCache,
    intrinsic: MarkdownIntrinsicCache,
    measure: Cpu,
}

impl Default for Harness {
    fn default() -> Self {
        Self {
            doc: PreviewDocument::default(),
            text: String::new(),
            view: View {
                scroll: 0.0,
                height: 600.0,
                padding: seats::preview_markdown_metrics(1.0).padding_y,
                end: false,
            },
            width: 800.0,
            scale: 1.0,
            offsets: Vec::new(),
            cache: preview_wrap::WindowCache::default(),
            intrinsic: MarkdownIntrinsicCache::default(),
            measure: Cpu::default(),
        }
    }
}

impl Harness {
    pub fn rebuild(&mut self, text: String, edits: Option<&[Edit]>, caret: Option<usize>) {
        self.rebuild_borrowed(&text, edits, caret);
        self.text = text;
    }

    pub fn rebuild_borrowed(&mut self, text: &str, edits: Option<&[Edit]>, caret: Option<usize>) {
        let old = std::mem::take(&mut self.doc);
        let timer = preview_typing::Timer::new("parse");
        let (blocks, ranges, maps) = preview::parse_markdown_mapped(text);
        drop(timer);
        let timer = preview_typing::Timer::new("formula discovery");
        std::hint::black_box(document_formulas(
            &blocks,
            seats::preview_markdown_metrics(self.scale),
        ));
        drop(timer);
        let timer = preview_typing::Timer::new("layout");
        let mut anchor = edits.and_then(|_| capture(&old, self.view, &mut self.measure));
        let source = caret.and_then(|at| {
            preview_typing::source_at(text, &blocks, &ranges, at).or_else(|| {
                let index = preview_live::caret_seat(text, &ranges, at).block()?;
                let raw = preview_live::block_source(text, &ranges[index]).to_owned();
                let metrics = seats::preview_text_metrics(self.scale);
                let advance = self.measure.width(
                    &[bt_render::PreviewRun {
                        text: "M".into(),
                        color: [0; 3],
                        mono: true,
                        bold: false,
                        italic: false,
                        font_scale: 1.0,
                        inline_box_px: None,
                    }],
                    metrics.font_size,
                    metrics.line_height,
                );
                Some(Box::new(MarkdownCaretBlock::Mono(MarkdownSourceBlock {
                    index,
                    range: ranges[index].clone(),
                    lines: preview_edit::display_lines(&raw),
                    text: raw,
                    font_size: metrics.font_size,
                    line_height: metrics.line_height,
                    advance,
                })))
            })
        });
        let frame = preview_wrap::Frame::new(self.width, self.scale, 0);
        let mut pass = self.cache.prepare(&old, edits.is_some(), frame);
        let (mut state, mut layout, mut intrinsic) = State::reconcile(Reconcile {
            old: &old,
            blocks: &blocks,
            ranges: &ranges,
            content: text,
            source: source.as_deref(),
            frame,
            edits,
            art_changed: false,
        });
        if let Some(anchor) = &mut anchor
            && let PreviewDocument::Markdown {
                ranges: old_ranges, ..
            } = &old
        {
            state.remap_anchor(anchor, old_ranges, &ranges, edits.unwrap_or_default());
            anchor.remap_text(
                source.as_deref(),
                &blocks,
                &ranges,
                &maps,
                edits.unwrap_or_default(),
            );
        }
        self.view.padding = frame.metrics().padding_y;
        let math = DocumentMath::default();
        let pictures = DocumentPictures::default();
        let palette = bt_render::chrome_palette();
        Realize {
            blocks: &blocks,
            source: source.as_deref(),
            art: PageArt {
                math: &math,
                pictures: &pictures,
                theme: bt_render::Theme::Dark,
            },
            layout: &mut layout,
            intrinsic: &mut intrinsic,
            state: &mut state,
            pass: &mut pass,
            bytes: MarkdownSourceBytes {
                content: text,
                ranges: &ranges,
            },
            intrinsic_pass: IntrinsicPass {
                metrics: frame.metrics(),
                math: &math,
                palette: &palette,
                scale_ppm: scale_ppm(self.scale),
                math_generation: 0,
            },
            cache: &mut self.intrinsic,
        }
        .ensure(&mut self.view, anchor, &mut self.measure);
        self.offsets = state.offsets(&self.offsets);
        let mut wrap = pass.document();
        Arc::make_mut(&mut wrap).viewport = state;
        self.doc = PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            source,
            intrinsic,
            layout,
            math,
            pictures,
            wrap,
        };
        drop(timer);
    }

    pub fn scroll(&mut self, scroll: f32, end: bool) {
        let text = std::mem::take(&mut self.text);
        self.scroll_bytes(scroll, end, &text);
        self.text = text;
    }

    pub fn scroll_bytes(&mut self, scroll: f32, end: bool, text: &str) {
        self.view.scroll = scroll;
        self.view.end = end;
        let PreviewDocument::Markdown {
            blocks,
            ranges,
            source,
            intrinsic,
            layout,
            math,
            pictures,
            wrap,
            ..
        } = &mut self.doc
        else {
            panic!()
        };
        let mut pass = self.cache.resume(wrap);
        let frame = wrap.frame.unwrap();
        let palette = bt_render::chrome_palette();
        if Arc::make_mut(wrap).viewport.pending(layout, self.view) {
            Realize {
                blocks,
                source: source.as_deref(),
                art: PageArt {
                    math,
                    pictures,
                    theme: bt_render::Theme::Dark,
                },
                layout,
                intrinsic,
                state: &mut Arc::make_mut(wrap).viewport,
                pass: &mut pass,
                bytes: MarkdownSourceBytes {
                    content: text,
                    ranges,
                },
                intrinsic_pass: IntrinsicPass {
                    metrics: frame.metrics(),
                    math,
                    palette: &palette,
                    scale_ppm: scale_ppm(self.scale),
                    math_generation: 0,
                },
                cache: &mut self.intrinsic,
            }
            .ensure(&mut self.view, None, &mut self.measure);
        }
    }

    pub fn layout(&self) -> &Layout {
        let PreviewDocument::Markdown { layout, .. } = &self.doc else {
            panic!()
        };
        layout
    }

    fn anchor(&self) -> Anchor {
        capture(&self.doc, self.view, &mut Cpu::default()).unwrap()
    }

    fn assert_exact(&self) {
        let PreviewDocument::Markdown { wrap, layout, .. } = &self.doc else {
            panic!()
        };
        assert!(!wrap.viewport.pending(layout, self.view));
    }
}

fn fixture(count: usize) -> String {
    (0..count)
        .map(|i| {
            format!(
                "Paragraph {i}: **bold** and `code` {}\n\n",
                "ordinary words ".repeat(15)
            )
        })
        .collect()
}

fn reset() {
    preview_typing::reset_work();
}
fn work(name: &'static str) -> usize {
    preview_typing::work(name)
}

/// Mutation: run the cold intrinsic pass across every table/fence, or shape all prose.
#[test]
fn viewport_open_bounds_prose_and_intrinsics() {
    for text in [fixture(12000),
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/DESIGN.md")).unwrap(),
        (0..4000).map(|i| format!("| name | value |\n| --- | --- |\n| item {i} | {i} |\n\n```rust\nlet x = {i};\n```\n\n")).collect(),
    ] {
        let mut h = Harness::default();
        reset();
        h.rebuild(text, None, None);
        h.assert_exact();
        let band = h.view.band(h.layout()).len();
        assert!(work("wrapped blocks") <= band + 1, "shaped {}, band {band}", work("wrapped blocks"));
        assert!(work("intrinsic blocks") <= band + 1);
        assert!(work("realized blocks") <= band + 1);
        assert!(work("wrap recipes") <= band + 1);
        assert!(work("realized blocks") < 80);
    }
}

/// Mutation: let parse-key equality bypass viewport realization, or visit all blocks.
#[test]
fn viewport_scroll_and_height_realize_only_new_blocks() {
    let mut h = Harness::default();
    h.rebuild(fixture(12000), None, None);
    let middle = h.layout().get(6000).unwrap().top;
    reset();
    h.scroll(middle, false);
    h.assert_exact();
    assert!(work("realized blocks") > 0 && work("realized blocks") < 80);
    assert!(work("wrap recipes") < 80);
    assert!(work("height tree updates") < work("realized blocks") * 16);
    reset();
    h.scroll(h.view.scroll, false);
    assert_eq!(work("realized blocks"), 0);
    h.view.height *= 2.0;
    h.scroll(h.view.scroll, false);
    h.assert_exact();
    assert!(work("realized blocks") > 0 && work("realized blocks") < 80);
}

/// Mutation: keep the stored pixel scroll after correcting estimates above it.
#[test]
fn viewport_corrections_above_preserve_anchor_and_caret_text() {
    let mut h = Harness::default();
    h.rebuild(fixture(12000), None, None);
    h.scroll(h.layout().get(6000).unwrap().top, false);
    let anchor = h.anchor();
    let PreviewDocument::Markdown { ranges, .. } = &h.doc else {
        panic!()
    };
    let at = ranges[anchor.index].start + 10;
    let text = h.text[at..at + 10].to_owned();
    let before = h.layout().get(anchor.index).unwrap().top + h.view.padding - h.view.scroll;
    let PreviewDocument::Markdown { layout, .. } = &mut h.doc else {
        panic!()
    };
    for i in [100, 200, 300] {
        let mut box_ = layout.get(i).unwrap();
        box_.height += 170.0;
        layout.set(i, box_);
    }
    // The production anchor restore used after each measurement batch.
    let local = match anchor.position {
        Position::Pixel(y) => y,
        _ => {
            let PreviewDocument::Markdown {
                blocks,
                source,
                wrap,
                math,
                pictures,
                ..
            } = &h.doc
            else {
                panic!()
            };
            let p = preview_wrap::anchor_paragraphs(
                &blocks[anchor.index],
                source.as_deref(),
                &h.layout().get(anchor.index).unwrap(),
                wrap.frame.unwrap(),
                PageArt {
                    math,
                    pictures,
                    theme: bt_render::Theme::Dark,
                },
            );
            anchor.local(&p, &mut |p| h.measure.rows(p))
        }
    };
    let PreviewDocument::Markdown { layout, .. } = &h.doc else {
        panic!()
    };
    anchor.restore(layout, local, &mut h.view);
    let after = h.layout().get(anchor.index).unwrap().top + h.view.padding - h.view.scroll;
    assert!((before - after).abs() < 0.1);
    assert_eq!(&h.text[at..at + 10], text);
}

/// Mutation: rebuild recipes for all unchanged occurrences on each keystroke.
#[test]
fn viewport_one_character_edit_builds_changed_and_visible_recipes() {
    let mut h = Harness::default();
    h.rebuild(fixture(12000), None, Some(12));
    let mut changed = h.text.clone();
    changed.insert(12, 'x');
    reset();
    h.rebuild(
        changed,
        Some(&[Edit {
            at: 12,
            removed: 0,
            inserted: 1,
        }]),
        Some(13),
    );
    h.assert_exact();
    assert_eq!(work("wrapped blocks"), 1);
    assert!(work("wrap recipes") <= h.view.band(h.layout()).len() + 4);
}

/// Mutation: use a content hash/old vector index to identify duplicate occurrences.
#[test]
fn viewport_duplicate_insertion_remaps_anchor_and_horizontal_offsets() {
    let duplicate = "| duplicate | value |\n| --- | --- |\n| repeated | wide |\n\n";
    let mut h = Harness::default();
    h.rebuild(
        format!("{}{}{}{}", fixture(100), duplicate, duplicate, fixture(100)),
        None,
        None,
    );
    h.offsets[100] = 12.0;
    h.offsets[101] = 37.0;
    h.scroll(h.layout().get(101).unwrap().top + h.view.padding, false);
    let before = h.anchor();
    let PreviewDocument::Markdown { ranges, .. } = &h.doc else {
        panic!()
    };
    let at = ranges[100].start;
    let mut changed = h.text.clone();
    changed.insert_str(at, duplicate);
    h.rebuild(
        changed,
        Some(&[Edit {
            at,
            removed: 0,
            inserted: duplicate.len(),
        }]),
        None,
    );
    let after = h.anchor();
    assert_eq!(before.id, after.id);
    assert_eq!(after.index, before.index + 1);
    assert!((before.screen_y - after.screen_y).abs() < 0.1);
    assert_eq!(&h.offsets[100..103], &[0.0, 12.0, 37.0]);
}

/// Mutation: reuse an old-width height as exact, or preserve only local pixels.
#[test]
fn viewport_width_and_scale_preserve_text_with_affinity() {
    let mut h = Harness::default();
    h.rebuild(fixture(12000), None, None);
    h.scroll(h.layout().get(6000).unwrap().top + h.view.padding, false);
    // Start inside a wrapped paragraph, at a soft-wrap seam.
    h.scroll(
        h.view.scroll + seats::preview_markdown_metrics(1.0).line_height,
        false,
    );
    let before = h.anchor();
    h.width = 330.0;
    reset();
    h.rebuild(h.text.clone(), Some(&[]), None);
    h.assert_exact();
    assert!(work("realized blocks") < 80);
    let after = h.anchor();
    assert_eq!(before.id, after.id);
    let (
        Position::Text {
            paragraph: p, byte, ..
        },
        Position::Text { paragraph: q, .. },
    ) = (&before.position, &after.position)
    else {
        panic!("text anchors");
    };
    assert_eq!(p, q);
    let PreviewDocument::Markdown {
        blocks,
        source,
        wrap,
        math,
        pictures,
        ..
    } = &h.doc
    else {
        panic!()
    };
    let paragraphs = preview_wrap::anchor_paragraphs(
        &blocks[after.index],
        source.as_deref(),
        &h.layout().get(after.index).unwrap(),
        wrap.frame.unwrap(),
        PageArt {
            math,
            pictures,
            theme: bt_render::Theme::Dark,
        },
    );
    let local = before.local(&paragraphs, &mut |p| h.measure.rows(p));
    let screen = h.view.padding + h.layout().get(after.index).unwrap().top + local - h.view.scroll;
    assert!(
        (screen - before.screen_y).abs() < 0.1,
        "byte {byte}, screen {screen}"
    );
    h.scale = 1.5;
    h.rebuild(h.text.clone(), Some(&[]), None);
    h.assert_exact();
    assert_eq!(h.anchor().id, before.id);
}

/// Mutation: clamp to the estimated end once, then let the final block grow away.
#[test]
fn viewport_end_navigation_finishes_at_real_final_block() {
    let mut h = Harness::default();
    h.rebuild(fixture(12000) + &"last words ".repeat(200), None, None);
    let end = h.layout().extent() + h.view.padding * 2.0 - h.view.height;
    reset();
    h.scroll(end, true);
    h.assert_exact();
    let bottom = h.view.padding + h.layout().extent() - h.view.scroll;
    assert!((bottom - (h.view.height - h.view.padding)).abs() < 0.1);
    assert!(work("realized blocks") < 80);
    let PreviewDocument::Markdown { wrap, .. } = &h.doc else {
        panic!()
    };
    assert!(wrap.viewport.records.last().unwrap().exact);
}

/// Mutation: leave an anchor naming a removed index or transfer its scroll offset.
#[test]
fn viewport_deleted_anchor_falls_forward_to_surviving_occurrence() {
    let mut h = Harness::default();
    h.rebuild(fixture(300), None, None);
    h.scroll(h.layout().get(150).unwrap().top + h.view.padding, false);
    let old = h.anchor();
    let PreviewDocument::Markdown { ranges, .. } = &h.doc else {
        panic!()
    };
    let cut = ranges[old.index].start..ranges[old.index + 1].start;
    let mut text = h.text.clone();
    text.replace_range(cut.clone(), "");
    h.rebuild(
        text,
        Some(&[Edit {
            at: cut.start,
            removed: cut.len(),
            inserted: 0,
        }]),
        None,
    );
    h.assert_exact();
    assert_ne!(h.anchor().id, old.id);
    assert_eq!(h.anchor().index, old.index);
}

/// Mutation: match occurrences only by their old end, or keep the old index
/// after an edit splits the paragraph holding the visible text.
#[test]
fn viewport_append_and_split_keep_the_anchored_source_text() {
    let mut h = Harness::default();
    let text = "ordinary words in a long paragraph ".repeat(500);
    h.rebuild(text, None, Some(0));
    h.scroll(900.0, false);
    let before = h.anchor();
    let at = h.text.len();
    let mut text = h.text.clone();
    text.push('x');
    h.rebuild(
        text,
        Some(&[Edit {
            at,
            removed: 0,
            inserted: 1,
        }]),
        Some(0),
    );
    assert_eq!(h.anchor().id, before.id);
    assert_eq!(h.anchor().file_byte, before.file_byte);
    assert!((h.view.scroll - 900.0).abs() < 0.1);

    let before = h.anchor();
    let at = 80;
    let mut text = h.text.clone();
    text.insert_str(at, "\n\n");
    h.rebuild(
        text,
        Some(&[Edit {
            at,
            removed: 0,
            inserted: 2,
        }]),
        Some(before.file_byte.unwrap() + 2),
    );
    let PreviewDocument::Markdown {
        blocks,
        source,
        wrap,
        math,
        pictures,
        ..
    } = &h.doc
    else {
        panic!()
    };
    let current = h.anchor();
    assert_eq!(
        current.index, 1,
        "fresh ranges find the text in the second fragment"
    );
    let mut mapped = before;
    mapped.index = current.index;
    let PreviewDocument::Markdown { ranges, maps, .. } = &h.doc else {
        panic!()
    };
    mapped.remap_text(
        source.as_deref(),
        blocks,
        ranges,
        maps,
        &[Edit {
            at,
            removed: 0,
            inserted: 2,
        }],
    );
    let paragraphs = preview_wrap::anchor_paragraphs(
        &blocks[current.index],
        source.as_deref(),
        &h.layout().get(current.index).unwrap(),
        wrap.frame.unwrap(),
        PageArt {
            math,
            pictures,
            theme: bt_render::Theme::Dark,
        },
    );
    let local = mapped.local(&paragraphs, &mut |p| h.measure.rows(p));
    let screen =
        h.view.padding + h.layout().get(current.index).unwrap().top + local - h.view.scroll;
    assert!((screen - mapped.screen_y).abs() < 0.1);
}

/// Mutation: measure all intrinsic widths on resize, or reuse a previous font
/// scale's widths merely because the source bytes still match.
#[test]
fn viewport_width_reuses_intrinsics_and_scale_invalidates_them() {
    let mut h = Harness::default();
    h.rebuild(
        "| name | value |\n| --- | --- |\n| wide text | 42 |\n\n```rust\nlet x = 42;\n```\n\n"
            .repeat(100),
        None,
        None,
    );
    h.width = 600.0;
    reset();
    h.rebuild(h.text.clone(), Some(&[]), None);
    assert_eq!(work("intrinsic blocks"), 0);
    assert_eq!(work("shaper calls"), 0);
    h.scale = 1.5;
    reset();
    h.rebuild(h.text.clone(), Some(&[]), None);
    assert!(work("intrinsic blocks") > 0);
    assert!(work("shaper calls") > 0);
    h.assert_exact();
}

/// Mutation: restore a raw fence/table anchor at local y=0 after its source
/// face closes, losing the source line or cell the reader was looking at.
#[test]
fn viewport_leaving_monospace_source_preserves_the_rendered_line() {
    for text in [
        format!(
            "```rust\n{}```\n\nafter\n",
            (0..120)
                .map(|i| format!("let x{i} = {i};\n"))
                .collect::<String>()
        ),
        format!(
            "| name | value |\n| --- | --- |\n{}\nafter\n",
            (0..120)
                .map(|i| format!("| item {i} | {i} |\n"))
                .collect::<String>()
        ),
    ] {
        let mut h = Harness::default();
        h.rebuild(text, None, Some(0));
        h.scroll(
            h.view.padding + seats::preview_text_metrics(1.0).line_height * 20.0,
            false,
        );
        let anchor = h.anchor();
        let byte = anchor.file_byte.unwrap();
        h.rebuild(h.text.clone(), Some(&[]), None);
        let PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            ..
        } = &h.doc
        else {
            panic!()
        };
        let place = preview_provenance::place_of(byte, blocks, ranges, maps).unwrap();
        let placed = h.layout().get(place.block).unwrap();
        let metrics = seats::preview_markdown_metrics(1.0);
        let local = match &blocks[place.block] {
            preview::MarkdownBlock::Code { .. } => {
                metrics.code_border
                    + metrics.code_padding_y
                    + place.piece as f32 * metrics.code_line_height
            }
            preview::MarkdownBlock::Table { .. } => {
                metrics.table_border
                    + metrics.table_padding_y
                    + placed.rows.iter().take(place.piece / 2).sum::<f32>()
            }
            _ => panic!(),
        };
        let screen = h.view.padding + placed.top + local - h.view.scroll;
        assert!(
            (screen - anchor.screen_y).abs() < 0.1,
            "source byte {byte} moved to {screen}"
        );
    }
}

/// Mutation: retain an old occurrence index after deleting the entire document.
#[test]
fn viewport_deleting_every_block_releases_anchor() {
    let mut h = Harness::default();
    h.rebuild(fixture(300), None, None);
    h.scroll(h.layout().get(150).unwrap().top + h.view.padding, false);
    let removed = h.text.len();
    h.rebuild(
        String::new(),
        Some(&[Edit {
            at: 0,
            removed,
            inserted: 0,
        }]),
        None,
    );
    assert_eq!(h.layout().len(), 0);
    assert_eq!(h.view.scroll, 0.0);
    assert!(h.offsets.is_empty());
    assert!(capture(&h.doc, h.view, &mut h.measure).is_none());
}

/// Mutation: retain a pixel y inside a rendered fence/table when its line
/// height changes, leaving a different source line under the viewport edge.
#[test]
fn viewport_scale_preserves_rendered_fence_and_table_text() {
    for text in [
        format!("```rust\n{}```\n", "let x = 42;\n".repeat(120)),
        format!(
            "| name | value |\n| --- | --- |\n{}",
            "| item | 42 |\n".repeat(120)
        ),
    ] {
        let mut h = Harness::default();
        h.rebuild(text, None, None);
        h.scroll(517.0, false);
        let anchor = h.anchor();
        assert!(anchor.file_byte.is_some(), "rendered row has provenance");
        let Position::Text {
            paragraph, within, ..
        } = anchor.position
        else {
            panic!("a scale correction needs a text position");
        };
        h.scale = 1.5;
        h.rebuild(h.text.clone(), Some(&[]), None);
        let PreviewDocument::Markdown { blocks, .. } = &h.doc else {
            panic!()
        };
        let placed = h.layout().get(0).unwrap();
        let local = fixed_text_y(
            &blocks[0],
            &placed,
            seats::preview_markdown_metrics(h.scale),
            paragraph,
            within,
        );
        let screen = h.view.padding + placed.top + local - h.view.scroll;
        assert!((screen - anchor.screen_y).abs() < 0.1);
    }
}
