//! Block-local Markdown wrapped measurements (DESIGN.md section 7.1.3x).

use crate::{
    MarkdownBlockIntrinsic, MarkdownBlockLayout, MarkdownCaretBlock, PageArt, PreviewDocument,
    WrapMeasure, markdown_prose_face, measure_markdown_local, preview, seats,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, Weak};

/// Disposable historical recipes and heights, shared by every pane in a window.
const WRAP_CACHE_BUDGET_BYTES: usize = 16 * 1024 * 1024;
/// Bump when aggregation or the fixed renderer shaping policy changes.
const LAYOUT_POLICY_EPOCH: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Representation {
    Rendered,
    RawProse,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum WrapPolicy {
    WordOrGlyph,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ShapingPolicy {
    Advanced,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum DirectionPolicy {
    Auto,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum FamilyRole {
    SansSerif,
    Monospace,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Weight {
    Normal,
    Semibold,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Style {
    Normal,
    Italic,
}

/// Full equality follows hashing: neither a digest nor a source slice is identity.
/// No occurrence offsets, origins, paint resources, margins or cumulative top.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct BlockWrapKey {
    representation: Representation,
    font_environment_epoch: u64,
    layout_policy_epoch: u64,
    scale_factor_bits: u32,
    paragraphs: Vec<ParagraphKey>,
    rows: bool,
    block_extra_height_px_bits: u32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ParagraphKey {
    effective_width_px_bits: u32,
    font_size_px_bits: u32,
    line_height_px_bits: u32,
    wrap: WrapPolicy,
    shaping: ShapingPolicy,
    letter_spacing_em_bits: u32,
    direction_policy: DirectionPolicy,
    runs: Vec<RunKey>,
    preceding_row_gap_px_bits: u32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RunKey {
    text: String,
    family_role: FamilyRole,
    weight: Weight,
    style: Style,
    font_scale_bits: u32,
    inline_box_width_px_bits: Option<u32>,
}

// Canonicalize signed zero; retain every other bit. Measurement replays these
// same floats, so no rounded lookup can stand for a different shaper input.
fn bits(value: f32) -> u32 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

impl RunKey {
    fn new(run: &bt_render::PreviewRun) -> Self {
        Self {
            text: run.text.clone(),
            family_role: if run.mono {
                FamilyRole::Monospace
            } else {
                FamilyRole::SansSerif
            },
            weight: if run.bold {
                Weight::Semibold
            } else {
                Weight::Normal
            },
            style: if run.italic {
                Style::Italic
            } else {
                Style::Normal
            },
            font_scale_bits: bits(run.font_scale),
            inline_box_width_px_bits: run.inline_box_px.map(bits),
        }
    }
    fn run(&self) -> bt_render::PreviewRun {
        bt_render::PreviewRun {
            text: self.text.clone(),
            color: [0; 3],
            mono: self.family_role == FamilyRole::Monospace,
            bold: self.weight == Weight::Semibold,
            italic: self.style == Style::Italic,
            font_scale: f32::from_bits(self.font_scale_bits),
            inline_box_px: self.inline_box_width_px_bits.map(f32::from_bits),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    width: f32,
    scale: f32,
    font_environment_epoch: u64,
}
impl Frame {
    pub(super) fn new(width: f32, scale: f32, font_environment_epoch: u64) -> Self {
        Self {
            width: width.max(1.0),
            scale,
            font_environment_epoch,
        }
    }
    fn metrics(self) -> seats::PreviewMarkdownMetrics {
        seats::preview_markdown_metrics(self.scale)
    }
}

/// Record the actual measurement recipe through the existing local layout
/// builder. A zero-height recorder leaves exactly the row gaps and block chrome.
/// This keeps markers, raw line boundaries, heading weights and resolved math
/// on the same path as uncached measurement, without storing a cosmic buffer.
fn recipe(
    block: &preview::MarkdownBlock,
    source: Option<&MarkdownCaretBlock>,
    frame: Frame,
    art: PageArt<'_>,
) -> Option<BlockWrapKey> {
    let representation = match source {
        Some(MarkdownCaretBlock::Prose(_)) => Representation::RawProse,
        Some(MarkdownCaretBlock::Mono(_)) => return None,
        None if markdown_prose_face(block).is_some() => Representation::Rendered,
        None => return None,
    };
    let paragraph_count = match source {
        Some(MarkdownCaretBlock::Prose(prose)) => prose.lines.len(),
        _ => match block {
            preview::MarkdownBlock::List { items, .. } => items.len(),
            preview::MarkdownBlock::Quote(lines) => lines.len(),
            _ => 1,
        },
    };
    let mut paragraphs = Vec::with_capacity(paragraph_count);
    let mut record = |runs: &[bt_render::PreviewRun], width: f32, font: f32, line: f32| {
        paragraphs.push(ParagraphKey {
            effective_width_px_bits: bits(width.max(1.0)),
            font_size_px_bits: bits(font),
            line_height_px_bits: bits(line),
            wrap: WrapPolicy::WordOrGlyph,
            shaping: ShapingPolicy::Advanced,
            letter_spacing_em_bits: bits(0.0),
            direction_policy: DirectionPolicy::Auto,
            runs: runs.iter().map(RunKey::new).collect(),
            preceding_row_gap_px_bits: bits(0.0),
        });
        0.0
    };
    let skeleton = measure_markdown_local(
        block,
        &MarkdownBlockIntrinsic::default(),
        source,
        frame.width,
        frame.metrics(),
        art,
        &mut record,
    );
    for (paragraph, gap) in paragraphs.iter_mut().zip(&skeleton.rows) {
        paragraph.preceding_row_gap_px_bits = bits(*gap);
    }
    Some(BlockWrapKey {
        representation,
        font_environment_epoch: frame.font_environment_epoch,
        layout_policy_epoch: LAYOUT_POLICY_EPOCH,
        scale_factor_bits: bits(frame.scale),
        paragraphs,
        rows: !skeleton.rows.is_empty(),
        block_extra_height_px_bits: bits(skeleton.height - skeleton.rows.iter().sum::<f32>()),
    })
}

#[derive(Clone, Debug)]
struct Measurement {
    height: f32,
    rows: Vec<f32>,
}
impl Measurement {
    fn layout(&self) -> MarkdownBlockLayout {
        MarkdownBlockLayout {
            height: self.height,
            rows: self.rows.clone(),
            ..MarkdownBlockLayout::default()
        }
    }
}
impl BlockWrapKey {
    fn measure(&self, measure: &mut WrapMeasure<'_>) -> Measurement {
        let rows: Vec<f32> = self
            .paragraphs
            .iter()
            .map(|p| {
                let runs: Vec<_> = p.runs.iter().map(RunKey::run).collect();
                f32::from_bits(p.preceding_row_gap_px_bits)
                    + measure(
                        &runs,
                        f32::from_bits(p.effective_width_px_bits),
                        f32::from_bits(p.font_size_px_bits),
                        f32::from_bits(p.line_height_px_bits),
                    )
            })
            .collect();
        Measurement {
            height: rows.iter().sum::<f32>() + f32::from_bits(self.block_extra_height_px_bits),
            rows: if self.rows { rows } else { Vec::new() },
        }
    }
    fn heap_bytes(&self) -> usize {
        self.paragraphs.capacity() * std::mem::size_of::<ParagraphKey>()
            + self
                .paragraphs
                .iter()
                .map(|p| {
                    p.runs.capacity() * std::mem::size_of::<RunKey>()
                        + p.runs.iter().map(|r| r.text.capacity()).sum::<usize>()
                })
                .sum::<usize>()
    }
}

#[derive(Debug, Eq, Hash, PartialEq)]
struct OwnedKey {
    owner: u64,
    recipe: BlockWrapKey,
}
#[derive(Debug)]
struct Entry {
    measurement: Measurement,
    used: u64,
}
#[derive(Debug)]
struct Cache {
    entries: HashMap<Arc<OwnedKey>, Entry>,
    lru: BTreeMap<u64, Arc<OwnedKey>>,
    budget: usize,
    tick: u64,
    next_owner: u64,
    payload_bytes: usize,
}
impl Cache {
    fn table_bytes(capacity: usize) -> usize {
        // Capacity counts usable slots, not buckets. Double it and include
        // control bytes/alignment to conservatively cover the table allocation.
        capacity * 2 * (std::mem::size_of::<(Arc<OwnedKey>, Entry)>() + 16)
    }

    fn bytes(&self) -> usize {
        self.payload_bytes + Self::table_bytes(self.entries.capacity())
            // Covers even a minimally occupied BTree node, including pointers.
            + self.lru.len() * 512
    }

    fn admission_bytes(&self, payload: usize) -> usize {
        let capacity = self.entries.capacity();
        let capacity = if self.entries.len() == capacity {
            // Leave room for table growth before insertion. Evicting first
            // avoids a grow/shrink/rehash cycle on every miss at the ceiling.
            ((capacity + 1) * 2).max(4)
        } else {
            capacity
        };
        self.payload_bytes + payload + Self::table_bytes(capacity) + (self.lru.len() + 1) * 512
    }

    fn payload(key: &OwnedKey, value: &Measurement) -> usize {
        (std::mem::size_of::<OwnedKey>() + 2 * std::mem::size_of::<usize>())
            + key.recipe.heap_bytes()
            + value.rows.capacity() * std::mem::size_of::<f32>()
    }
    fn remove(&mut self, key: &OwnedKey) {
        if let Some((key, entry)) = self.entries.remove_entry(key) {
            self.payload_bytes -= Self::payload(&key, &entry.measurement);
            self.lru.remove(&entry.used);
            if self.entries.is_empty() {
                self.entries.shrink_to_fit();
                // An empty BTree may retain its root allocation; release it too.
                self.lru = BTreeMap::new();
            }
        }
    }
    fn lookup(&mut self, key: &OwnedKey) -> Option<Measurement> {
        self.tick += 1;
        let entry = self.entries.get_mut(key)?;
        let key = self
            .lru
            .remove(&entry.used)
            .expect("each entry has a recency key");
        entry.used = self.tick;
        self.lru.insert(self.tick, key);
        Some(entry.measurement.clone())
    }
    fn insert(&mut self, key: OwnedKey, measurement: Measurement) {
        let payload = Self::payload(&key, &measurement);
        // An oversized recipe gets current-layout reuse, never admission that
        // flushes everybody else's history before rejecting the new entry.
        if payload + 512 + 8 * (std::mem::size_of::<(Arc<OwnedKey>, Entry)>() + 16) > self.budget {
            return;
        }
        self.remove(&key);
        while self.admission_bytes(payload) > self.budget {
            let oldest = self.lru.first_key_value().map(|(_, key)| key.clone());
            let Some(oldest) = oldest else {
                return;
            };
            self.remove(&oldest);
        }
        self.tick += 1;
        self.payload_bytes += payload;
        let key = Arc::new(key);
        self.lru.insert(self.tick, key.clone());
        self.entries.insert(
            key,
            Entry {
                measurement,
                used: self.tick,
            },
        );
        while self.bytes() > self.budget {
            let oldest = self.lru.first_key_value().map(|(_, key)| key.clone());
            let Some(oldest) = oldest else {
                break;
            };
            self.remove(&oldest);
        }
    }
    fn release(&mut self, owner: u64) {
        let owned: Vec<_> = self
            .entries
            .keys()
            .filter(|key| key.owner == owner)
            .cloned()
            .collect();
        for key in owned {
            self.remove(&key);
        }
        self.entries.shrink_to_fit();
    }
}

#[derive(Clone, Debug)]
pub(super) struct WindowCache(Arc<Mutex<Cache>>);
impl Default for WindowCache {
    fn default() -> Self {
        Self::with_budget(WRAP_CACHE_BUDGET_BYTES)
    }
}
impl WindowCache {
    fn with_budget(budget: usize) -> Self {
        Self(Arc::new(Mutex::new(Cache {
            entries: HashMap::new(),
            lru: BTreeMap::new(),
            budget,
            tick: 0,
            next_owner: 0,
            payload_bytes: 0,
        })))
    }
    #[cfg(test)]
    fn bytes(&self) -> usize {
        self.0.lock().unwrap().bytes()
    }
    pub(super) fn prepare(&self, doc: &PreviewDocument, same_document: bool, frame: Frame) -> Pass {
        let previous = if same_document {
            standing(doc)
        } else {
            HashMap::new()
        };
        let existing = match doc {
            PreviewDocument::Markdown { wrap, .. } if same_document => wrap.owner.clone(),
            _ => None,
        };
        let owner = existing
            .filter(|o| o.cache.ptr_eq(&Arc::downgrade(&self.0)))
            .unwrap_or_else(|| {
                let mut cache = self.0.lock().unwrap();
                cache.next_owner += 1;
                Arc::new(Owner {
                    id: cache.next_owner,
                    cache: Arc::downgrade(&self.0),
                })
            });
        Pass {
            previous,
            owner,
            frame,
        }
    }
}
#[derive(Debug)]
struct Owner {
    id: u64,
    cache: Weak<Mutex<Cache>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(cache) = self.cache.upgrade() {
            cache.lock().unwrap().release(self.id);
        }
    }
}
/// Only a lease and the old frame's recipe environment persist beside the
/// parsed document. Its existing layout is the compact current measurement
/// record. Full keys for current reuse are reconstructed transiently from that
/// SAME parse; no index-keyed state is ever attached to a new parse.
#[derive(Clone, Debug, Default)]
pub(super) struct Document {
    owner: Option<Arc<Owner>>,
    frame: Option<Frame>,
}

fn standing(doc: &PreviewDocument) -> HashMap<BlockWrapKey, Measurement> {
    let PreviewDocument::Markdown {
        blocks,
        source,
        layout,
        math,
        pictures,
        wrap,
        ..
    } = doc
    else {
        return HashMap::new();
    };
    let Some(frame) = wrap.frame else {
        return HashMap::new();
    };
    let art = PageArt {
        math,
        pictures,
        theme: bt_render::Theme::Dark,
    };
    blocks
        .iter()
        .zip(layout)
        .enumerate()
        .filter_map(|(index, (block, placed))| {
            let source = source.as_deref().filter(|s| s.index() == index);
            Some((
                recipe(block, source, frame, art)?,
                Measurement {
                    height: placed.height,
                    rows: placed.rows.clone(),
                },
            ))
        })
        .collect()
}

pub(super) struct Pass {
    previous: HashMap<BlockWrapKey, Measurement>,
    owner: Arc<Owner>,
    frame: Frame,
}
impl Pass {
    pub(super) fn document(&self) -> Arc<Document> {
        Arc::new(Document {
            owner: Some(self.owner.clone()),
            frame: Some(self.frame),
        })
    }
    fn measure(&mut self, key: BlockWrapKey, measure: &mut WrapMeasure<'_>) -> MarkdownBlockLayout {
        let owned = OwnedKey {
            owner: self.owner.id,
            recipe: key,
        };
        let shared = self
            .owner
            .cache
            .upgrade()
            .expect("window outlives layout pass");
        if let Some(found) = shared.lock().unwrap().lookup(&owned) {
            return found.layout();
        }
        let value = self
            .previous
            .get(&owned.recipe)
            .cloned()
            .unwrap_or_else(|| owned.recipe.measure(measure));
        let layout = value.layout();
        shared.lock().unwrap().insert(owned, value);
        layout
    }
}

/// Same eager traversal and margin accumulation as before, with local reuse.
/// Width-independent table/code measurements and arithmetic mono wraps stay on
/// their existing paths; only rendered prose and raw proportional lines cache.
pub(super) fn lay_markdown_out_cached(
    blocks: &[preview::MarkdownBlock],
    intrinsic: &[MarkdownBlockIntrinsic],
    source: Option<&MarkdownCaretBlock>,
    art: PageArt<'_>,
    pass: &mut Pass,
    measure: &mut WrapMeasure<'_>,
) -> Vec<MarkdownBlockLayout> {
    let metrics = pass.frame.metrics();
    let mut top = 0.0_f32;
    let mut previous_bottom = 0.0_f32;
    let mut previous = None;
    blocks
        .iter()
        .zip(intrinsic)
        .enumerate()
        .map(|(index, (block, intrinsic))| {
            let source = source.filter(|s| s.index() == index);
            let mut measured = if let Some(key) = recipe(block, source, pass.frame, art) {
                pass.measure(key, measure)
            } else {
                measure_markdown_local(
                    block,
                    intrinsic,
                    source,
                    pass.frame.width,
                    metrics,
                    art,
                    measure,
                )
            };
            let (margin_top, margin_bottom) =
                preview::markdown_block_margins(block, previous, metrics);
            top += previous_bottom.max(margin_top);
            measured.top = top;
            top += measured.height;
            previous_bottom = margin_bottom;
            previous = Some(block);
            measured
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[derive(Default)]
    struct Harness {
        cache: WindowCache,
        doc: PreviewDocument,
        calls: usize,
        font_epoch: u64,
    }

    impl Harness {
        fn step(
            &mut self,
            text: &str,
            caret: Option<usize>,
            width: f32,
            scale: f32,
            math: DocumentMath,
        ) {
            let frame = Frame::new(width, scale, self.font_epoch);
            let mut pass = self.cache.prepare(&self.doc, true, frame);
            let (blocks, ranges, maps) = preview::parse_markdown_mapped(text);
            // Deliberately use the NEW ranges and blocks, just like the runtime.
            let source = caret.and_then(|at| {
                let index = preview_live::caret_seat(text, &ranges, at).block()?;
                let heading = markdown_prose_face(&blocks[index])?;
                let raw = preview_live::block_source(text, &ranges[index]).to_owned();
                let metrics = frame.metrics();
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
                    lines: prose_source_lines(&raw),
                    text: raw,
                    heading: heading.is_some(),
                    font_size,
                    line_height,
                })))
            });
            let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
            let pictures = DocumentPictures::default();
            let art = PageArt {
                math: &math,
                pictures: &pictures,
                theme: bt_render::Theme::Dark,
            };
            let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
                self.calls += 1;
                let ink: f32 = runs
                    .iter()
                    .map(|r| {
                        r.inline_box_px
                            .unwrap_or(r.text.len() as f32 * if r.bold { 9.0 } else { 8.0 })
                    })
                    .sum();
                (ink / width).ceil().max(1.0) * line
            };
            let layout = lay_markdown_out_cached(
                &blocks,
                &intrinsic,
                source.as_deref(),
                art,
                &mut pass,
                &mut shaper,
            );
            self.doc = PreviewDocument::Markdown {
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
        fn keys(&self) -> Vec<BlockWrapKey> {
            standing(&self.doc).into_keys().collect()
        }
    }

    fn step(h: &mut Harness, text: &str, caret: Option<usize>) {
        h.step(text, caret, 400.0, 1.0, DocumentMath::default());
    }

    /// Mutation: key by the source slice, or omit styled run text; emphasis is
    /// resolved across the image before the parser splits the paragraph.
    #[test]
    fn emphasis_across_image_has_distinct_recipe_keys() {
        let a = "**a ![x](x) b**";
        let b = "**a ![x](x) b";
        let (_, ar) = preview::parse_markdown_ranged(a);
        let (_, br) = preview::parse_markdown_ranged(b);
        assert_eq!(&a[ar[0].clone()], &b[br[0].clone()]);
        let mut h = Harness::default();
        step(&mut h, a, None);
        let old = h.keys();
        step(&mut h, b, None);
        assert!(old.iter().all(|key| !h.keys().contains(key)));
    }

    /// Mutation: attach an index-keyed old measurement vector to the new parse;
    /// insertion before duplicate paragraphs must retain fresh caret/range/maps.
    #[test]
    fn insertion_before_duplicate_blocks_uses_new_occurrences() {
        let mut h = Harness::default();
        step(&mut h, "same **words**\n\nsame **words**", Some(17));
        let before = h.calls;
        let text = "new heading\n\nsame **words**\n\nsame **words**";
        step(&mut h, text, Some(text.rfind("same").unwrap()));
        assert_eq!(h.calls - before, 1);
        let PreviewDocument::Markdown {
            blocks,
            source,
            ranges,
            maps,
            ..
        } = &h.doc
        else {
            panic!()
        };
        assert_eq!(source.as_ref().unwrap().index(), 2);
        assert_eq!(ranges[2].start, text.rfind("same").unwrap());
        assert_eq!(maps.len(), ranges.len());
        assert_eq!(
            preview_provenance::file_offset_of(
                &preview_select::Place::new(2, 0, 0),
                blocks,
                ranges,
                maps
            ),
            Some(ranges[2].start)
        );
    }

    /// Mutation: bypass wrapped reuse or omit representation from the key.
    #[test]
    fn cross_block_caret_flip_measures_only_two_blocks() {
        let mut h = Harness::default();
        let text = "first **bold**\n\nsecond *italic*\n\nthird stable";
        step(&mut h, text, Some(0));
        let before = h.calls;
        step(&mut h, text, Some(text.find("second").unwrap()));
        assert_eq!(h.calls - before, 2);
        let before = h.calls;
        step(&mut h, text, Some(0));
        assert_eq!(h.calls - before, 0, "both faces are now historical hits");
    }

    /// Mutation: round effective widths or omit explicit scale/epoch fields.
    #[test]
    fn width_scale_and_environment_changes_miss() {
        let mut h = Harness::default();
        step(&mut h, "words", None);
        let before = h.calls;
        h.step("words", None, 400.25, 1.0, DocumentMath::default());
        h.step("words", None, 400.25, 1.5, DocumentMath::default());
        h.font_epoch += 1;
        h.step("words", None, 400.25, 1.5, DocumentMath::default());
        assert_eq!(h.calls - before, 3);
        let key = h.keys().pop().unwrap();
        let mut changed = key.clone();
        changed.font_environment_epoch += 1;
        assert_ne!(key, changed);
        changed = key.clone();
        changed.layout_policy_epoch += 1;
        assert_ne!(key, changed);
        changed = key.clone();
        changed.scale_factor_bits = 1.0_f32.to_bits();
        assert_ne!(key, changed);
        let buffer = preview::PreviewBuffer::new(
            preview::PreviewSource::File(PathBuf::from("test.md")),
            "test.md".into(),
        );
        let a = preview_document_key(
            &buffer,
            false,
            400.0,
            1.0,
            PageArtKey {
                math_generation: 0,
                body_ink: [0; 3],
                picture_generation: 0,
                picture_reach: PictureReach::from_the_top(),
                theme: bt_render::Theme::Dark,
            },
            None,
        );
        let b = preview_document_key(
            &buffer,
            false,
            400.25,
            1.0,
            PageArtKey {
                math_generation: 0,
                body_ink: [0; 3],
                picture_generation: 0,
                picture_reach: PictureReach::from_the_top(),
                theme: bt_render::Theme::Dark,
            },
            None,
        );
        assert_ne!(
            a, b,
            "the outer early return must see fractional widths too"
        );
    }

    /// Mutation: use global math generation, or omit resolved inline-box width.
    #[test]
    fn math_arrival_invalidates_only_affected_blocks() {
        let mut h = Harness::default();
        let text = "plain words\n\nformula $x$\n\nmore plain words";
        step(&mut h, text, None);
        let before = h.calls;
        let mut math = DocumentMath::default();
        math.inline
            .entry(math_em_milli(
                seats::preview_markdown_metrics(1.0).font_size,
            ))
            .or_default()
            .insert(
                "x".to_owned(),
                PreviewMathPicture {
                    key: "test-formula".into(),
                    baseline_px: 16.0,
                    rgba: std::sync::Arc::from(vec![0u8; 4]),
                    width_px: 80,
                    height_px: 20,
                },
            );
        h.step(text, None, 400.0, 1.0, math);
        assert_eq!(h.calls - before, 1);
    }

    /// Mutation: include selection, caret byte offset, paint colour or IME
    /// preedit in committed measurement inputs, instead of passing None.
    #[test]
    fn selection_and_ime_preedit_do_not_touch_key() {
        let mut h = Harness::default();
        step(&mut h, "**hello** world", Some(0));
        let old = h.keys();
        let mut pane = PreviewPane {
            doc: h.doc.clone(),
            ..PreviewPane::default()
        };
        pane.caret.anchor = 2;
        pane.caret.caret = 5;
        let PreviewDocument::Markdown {
            source: Some(source),
            ..
        } = &pane.doc
        else {
            panic!()
        };
        let MarkdownCaretBlock::Prose(prose) = source.as_ref() else {
            panic!()
        };
        let preedit = MarkdownPreedit {
            text: "composing many words".into(),
            caret_byte: 3,
        };
        let paint = markdown_prose_runs(
            prose,
            0,
            Some((5, &preedit.text)),
            &bt_render::chrome_palette(),
        );
        assert!(paint.iter().any(|run| run.text == preedit.text));
        assert_eq!(standing(&pane.doc).into_keys().collect::<Vec<_>>(), old);
        let before = h.calls;
        step(&mut h, "**hello** world", Some(5));
        assert_eq!(h.calls, before);
    }

    /// Mutation: leave md_block_scroll indexed by the previous parse in
    /// show_document, especially across its Reparse::Ours early return.
    #[test]
    fn reparse_resets_horizontal_scroll_occurrences() {
        let table = "a | b\n--- | ---\nx | y";
        let mut h = Harness::default();
        step(&mut h, &format!("{table}\n\n{table}"), None);
        let mut pane = PreviewPane {
            doc: h.doc.clone(),
            md_block_scroll: vec![0.0, 87.0],
            ..PreviewPane::default()
        };
        pane.reflow_document(h.doc.clone());
        assert_eq!(pane.md_block_scroll, vec![0.0, 87.0]);
        step(&mut h, &format!("inserted\n\n{table}\n\n{table}"), None);
        pane.show_document(h.doc.clone(), Reparse::Ours);
        assert!(pane.md_block_scroll.is_empty());
        pane.md_block_scroll = vec![87.0];
        pane.show_document(h.doc, Reparse::Elsewhere);
        assert!(pane.md_block_scroll.is_empty());
    }

    /// Mutation: let LRU evictions remove the only copy of current geometry;
    /// even a zero-byte historical budget must reuse current parsed recipes.
    #[test]
    fn tiny_budget_keeps_current_measurements_and_drop_releases_history() {
        let mut h = Harness {
            cache: WindowCache::with_budget(0),
            ..Harness::default()
        };
        step(&mut h, "one\n\ntwo\n\nthree", None);
        let before = h.calls;
        step(&mut h, "one\n\ntwo\n\nthree", None);
        assert_eq!(h.calls, before);
        assert_eq!(h.cache.bytes(), 0);
        let mut h = Harness::default();
        step(&mut h, "owned entry", None);
        assert!(h.cache.bytes() > 0);
        h.doc = PreviewDocument::Empty;
        assert_eq!(h.cache.bytes(), 0);
    }

    /// Mutation: touch recency only once per pass, ignore allocation capacity,
    /// or admit an oversized entry by flushing the entire cache first.
    #[test]
    fn byte_budget_evicts_the_least_recent_access() {
        let mut h = Harness::default();
        step(&mut h, "a\n\nb\n\nc", None);
        let keys = h.keys();
        let owner = match &h.doc {
            PreviewDocument::Markdown { wrap, .. } => wrap.owner.as_ref().unwrap().id,
            _ => panic!(),
        };
        let mut cache = h.cache.0.lock().unwrap();
        cache.entries.clear();
        cache.lru.clear();
        cache.payload_bytes = 0;
        let value = || Measurement {
            height: 20.0,
            rows: Vec::new(),
        };
        for key in &keys {
            cache.insert(
                OwnedKey {
                    owner,
                    recipe: key.clone(),
                },
                value(),
            );
        }
        cache.budget = cache.bytes();
        cache
            .lookup(&OwnedKey {
                owner,
                recipe: keys[0].clone(),
            })
            .unwrap();
        let mut fourth = keys[0].clone();
        fourth.paragraphs[0].runs[0].text = "d".into();
        cache.insert(
            OwnedKey {
                owner,
                recipe: fourth,
            },
            value(),
        );
        assert!(cache.bytes() <= cache.budget);
        assert!(
            cache
                .lookup(&OwnedKey {
                    owner,
                    recipe: keys[0].clone()
                })
                .is_some()
        );
        assert!(
            cache
                .lookup(&OwnedKey {
                    owner,
                    recipe: keys[1].clone()
                })
                .is_none()
        );
        let before = cache.bytes();
        let mut huge = keys[0].clone();
        huge.paragraphs[0].runs[0].text = "huge".repeat(cache.budget);
        cache.insert(
            OwnedKey {
                owner,
                recipe: huge,
            },
            value(),
        );
        assert_eq!(cache.bytes(), before);
    }

    /// Mutation: omit any metric-bearing run attribute from canonicalization.
    /// Colours must be excluded, while directional controls remain literal text.
    #[test]
    fn every_run_metric_is_part_of_the_recipe() {
        let mut run = bt_render::PreviewRun {
            text: "abc".into(),
            color: [0; 3],
            mono: false,
            bold: false,
            italic: false,
            font_scale: 1.0,
            inline_box_px: None,
        };
        let base = RunKey::new(&run);
        run.color = [255; 3];
        assert_eq!(base, RunKey::new(&run));
        for mutate in [
            (|r: &mut bt_render::PreviewRun| r.text.push('\u{202e}'))
                as fn(&mut bt_render::PreviewRun),
            |r| r.mono = true,
            |r| r.bold = true,
            |r| r.italic = true,
            |r| r.font_scale = 0.85,
            |r| r.inline_box_px = Some(18.25),
        ] {
            let mut changed = run.clone();
            mutate(&mut changed);
            assert_ne!(base, RunKey::new(&changed));
        }
    }

    /// Mutation: cache margins/top, lose list row gaps or quote padding, or
    /// use pane width instead of the indented paragraph measure width.
    #[test]
    fn recipes_preserve_row_geometry_and_recompute_neighbour_margins() {
        let mut h = Harness::default();
        let text = "# Heading\n\n- first item\n- second item\n\n> quote\n> next\n\nparagraph";
        step(&mut h, text, None);
        let keys = h.keys();
        let metrics = seats::preview_markdown_metrics(1.0);
        let list = keys
            .iter()
            .find(|k| k.paragraphs.len() == 2 && k.paragraphs[1].preceding_row_gap_px_bits != 0)
            .unwrap();
        assert_eq!(
            f32::from_bits(list.paragraphs[0].effective_width_px_bits),
            400.0 - metrics.list_indent
        );
        assert_eq!(
            f32::from_bits(list.paragraphs[1].preceding_row_gap_px_bits),
            metrics.list_item_gap
        );
        let quote = keys
            .iter()
            .find(|k| k.rows && k.block_extra_height_px_bits != 0)
            .unwrap();
        assert_eq!(
            f32::from_bits(quote.block_extra_height_px_bits),
            metrics.quote_padding_y * 2.0
        );
        assert_eq!(
            f32::from_bits(quote.paragraphs[0].effective_width_px_bits),
            400.0 - metrics.quote_indent
        );
        let before = h.calls;
        step(&mut h, &format!("intro\n\n{text}"), None);
        assert_eq!(h.calls - before, 1);
        let PreviewDocument::Markdown { layout, .. } = &h.doc else {
            panic!()
        };
        assert!(layout[1].top > 0.0);
    }

    /// Mutation: alter cached row aggregation, raw source-line boundaries,
    /// generated markers or heading chrome relative to uncached measurement.
    #[test]
    fn cached_geometry_matches_uncached_prose_and_raw_lines() {
        let text = "# Heading\n\nfirst **bold** and `code`\nsecond source line\n\n3. first item\n4. second item\n\n> quote\n> next";
        let mut h = Harness::default();
        for caret in [
            None,
            Some(0),
            Some(text.find("first **").unwrap()),
            Some(text.find("3.").unwrap()),
            Some(text.find('>').unwrap()),
            None,
        ] {
            step(&mut h, text, caret);
            let PreviewDocument::Markdown {
                blocks,
                intrinsic,
                source,
                math,
                pictures,
                layout,
                ..
            } = &h.doc
            else {
                panic!()
            };
            let mut shape = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
                let ink: f32 = runs
                    .iter()
                    .map(|r| {
                        r.inline_box_px
                            .unwrap_or(r.text.len() as f32 * if r.bold { 9.0 } else { 8.0 })
                    })
                    .sum();
                (ink / width).ceil().max(1.0) * line
            };
            let baseline = lay_markdown_out(
                blocks,
                intrinsic,
                source.as_deref(),
                400.0,
                seats::preview_markdown_metrics(1.0),
                PageArt {
                    math,
                    pictures,
                    theme: bt_render::Theme::Dark,
                },
                &mut shape,
            );
            assert_eq!(*layout, baseline);
        }
    }

    fn uncached_calls(doc: &PreviewDocument) -> usize {
        let PreviewDocument::Markdown {
            blocks,
            intrinsic,
            source,
            math,
            pictures,
            ..
        } = doc
        else {
            panic!()
        };
        let mut calls = 0;
        let mut measure = |_: &[bt_render::PreviewRun], _: f32, _: f32, line: f32| {
            calls += 1;
            line
        };
        lay_markdown_out(
            blocks,
            intrinsic,
            source.as_deref(),
            400.0,
            seats::preview_markdown_metrics(1.0),
            PageArt {
                math,
                pictures,
                theme: bt_render::Theme::Dark,
            },
            &mut measure,
        );
        calls
    }

    /// Mutation: call the shaper for every prose block on caret movement/edit.
    /// Non-GUI timing probe: elapsed time is diagnostic; acceptance counts calls.
    #[test]
    fn large_document_timing_counts_shaper_calls() {
        let text: String = (0..12000).map(|i| format!("Paragraph {i}: **bold text** with `inline code` and enough ordinary words to wrap. {}\n\n", "prose ".repeat(32))).collect();
        assert!(text.len() > 3_200_000);
        let mut h = Harness::default();
        let clock = Instant::now();
        step(&mut h, &text, Some(0));
        let cold = h.calls;
        let before_cold = uncached_calls(&h.doc);
        let caret = text.find("Paragraph 1:").unwrap();
        step(&mut h, &text, Some(caret));
        let flip = h.calls - cold;
        let before_flip = uncached_calls(&h.doc);
        let mut edited = text.clone();
        edited.insert(caret + 1, 'X');
        let before = h.calls;
        step(&mut h, &edited, Some(caret + 1));
        let edit = h.calls - before;
        let before_edit = uncached_calls(&h.doc);
        println!(
            "wrapped timing: bytes={} blocks=12000 before cold/flip/edit={before_cold}/{before_flip}/{before_edit} after={cold}/{flip}/{edit} elapsed={:?}",
            text.len(),
            clock.elapsed()
        );
        assert_eq!(
            (before_cold, before_flip, before_edit),
            (12000, 12000, 12000)
        );
        assert_eq!(cold, 12000);
        assert_eq!(flip, 2);
        assert_eq!(edit, 1);
    }
}
