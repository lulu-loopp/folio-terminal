use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    time::Instant,
};

use anyhow::{Context, Result, bail};
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, Wrap};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub const LOG_SCHEMA: &str = "bt-ime-probe/v1";
pub const CELL_WIDTH_PX: f32 = 14.0;
pub const FONT_SIZE_PX: f32 = 20.0;
pub const LINE_HEIGHT_PX: f32 = 28.0;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WidthCase {
    pub id: String,
    pub text: String,
    pub expected_cells: usize,
    pub expected_cells_source: String,
    pub expected_cells_bt_term: usize,
    pub expected_cells_bt_term_source: String,
    pub class: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClusterAudit {
    pub text: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub candidate_cells: usize,
    pub slot_start_px: f32,
    pub candidate_slot_width_px: f32,
    pub natural_start_px: f32,
    pub natural_advance_px: f32,
    pub constrained_offset_px: f32,
    pub clipped: bool,
    pub glyph_count: usize,
    pub missing_glyphs: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShapeCaseAudit {
    pub id: String,
    pub text: String,
    pub class: String,
    pub expected_cells: usize,
    pub expected_cells_source: String,
    pub expected_cells_bt_term: usize,
    pub expected_cells_bt_term_source: String,
    pub candidate_cells: usize,
    pub natural_line_width_px: f32,
    pub candidate_slot_width_px: f32,
    pub shape_micros: u128,
    pub clusters: Vec<ClusterAudit>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShapeAuditReport {
    pub schema: &'static str,
    pub cosmic_text_version: &'static str,
    pub font_initialization_micros: u128,
    pub cell_width_px: f32,
    pub font_size_px: f32,
    pub line_height_px: f32,
    pub cases: Vec<ShapeCaseAudit>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LogRecord {
    pub schema: String,
    pub sequence: u64,
    pub elapsed_micros: u128,
    pub ime_name: String,
    pub event: String,
    pub payload: Value,
}

pub struct JsonlLogger {
    writer: BufWriter<File>,
    started: Instant,
    sequence: u64,
    ime_name: String,
}

impl JsonlLogger {
    pub fn create(path: &Path, ime_name: String) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create log directory {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!(
                    "create new log {} (choose another name if it already exists)",
                    path.display()
                )
            })?;
        Ok(Self {
            writer: BufWriter::new(file),
            started: Instant::now(),
            sequence: 0,
            ime_name,
        })
    }

    pub fn emit(&mut self, event: &str, payload: Value) -> Result<()> {
        let record = LogRecord {
            schema: LOG_SCHEMA.to_owned(),
            sequence: self.sequence,
            elapsed_micros: self.started.elapsed().as_micros(),
            ime_name: self.ime_name.clone(),
            event: event.to_owned(),
            payload,
        };
        self.sequence += 1;
        serde_json::to_writer(&mut self.writer, &record).context("serialize IME log record")?;
        self.writer
            .write_all(b"\n")
            .context("write IME log newline")?;
        self.writer.flush().context("flush IME log")
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LogAudit {
    pub records: usize,
    pub frames: usize,
    pub set_area_calls: usize,
    pub ime_enabled: usize,
    pub nonempty_preedits: usize,
    pub nonempty_commits: usize,
    pub distinct_candidate_areas: usize,
    pub checklist_items: Vec<u8>,
    pub failures: Vec<String>,
}

pub fn audit_log(path: &Path, strict_ime: bool) -> Result<LogAudit> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut audit = LogAudit::default();
    let mut areas = BTreeSet::new();
    let mut checklist_items = BTreeSet::new();
    let mut saw_boot = false;
    let mut saw_shutdown = false;

    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("read log line {}", line_index + 1))?;
        let record: LogRecord = serde_json::from_str(&line)
            .with_context(|| format!("parse log line {}", line_index + 1))?;
        if record.schema != LOG_SCHEMA {
            audit.failures.push(format!(
                "line {} has schema {}, expected {}",
                line_index + 1,
                record.schema,
                LOG_SCHEMA
            ));
        }
        if record.sequence != audit.records as u64 {
            audit.failures.push(format!(
                "line {} sequence {}, expected {}",
                line_index + 1,
                record.sequence,
                audit.records
            ));
        }
        audit.records += 1;
        match record.event.as_str() {
            "boot" => saw_boot = true,
            "shutdown" => saw_shutdown = true,
            "frame" => audit.frames += 1,
            "set_ime_cursor_area" => {
                audit.set_area_calls += 1;
                if let Some(area) = record.payload.get("area") {
                    areas.insert(area.to_string());
                }
            }
            "ime_enabled" => audit.ime_enabled += 1,
            "ime_preedit" => {
                if let Some(text) = record.payload.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        audit.nonempty_preedits += 1;
                    }
                    validate_cursor_range(
                        text,
                        &record.payload,
                        &mut audit.failures,
                        line_index + 1,
                    );
                }
            }
            "ime_commit" => {
                if record
                    .payload
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty())
                {
                    audit.nonempty_commits += 1;
                }
            }
            "checklist_item" => match record.payload.get("item").and_then(Value::as_u64) {
                Some(item @ 1..=10) => {
                    checklist_items.insert(item as u8);
                }
                _ => audit.failures.push(format!(
                    "line {} has invalid checklist item; expected integer 1..=10",
                    line_index + 1
                )),
            },
            _ => {}
        }
    }
    audit.distinct_candidate_areas = areas.len();
    audit.checklist_items = checklist_items.iter().copied().collect();
    if !saw_boot {
        audit.failures.push("missing boot record".to_owned());
    }
    if !saw_shutdown {
        audit.failures.push("missing shutdown record".to_owned());
    }
    if audit.frames == 0 {
        audit
            .failures
            .push("no rendered frame was logged".to_owned());
    }
    if audit.set_area_calls == 0 {
        audit
            .failures
            .push("set_ime_cursor_area was never called".to_owned());
    }
    if strict_ime {
        if audit.ime_enabled == 0 {
            audit.failures.push("no Ime::Enabled event".to_owned());
        }
        if audit.nonempty_preedits == 0 {
            audit
                .failures
                .push("no non-empty Ime::Preedit event".to_owned());
        }
        if audit.nonempty_commits == 0 {
            audit
                .failures
                .push("no non-empty Ime::Commit event".to_owned());
        }
        if audit.distinct_candidate_areas < 2 {
            audit.failures.push(
                "candidate area never moved; cursor-follow behavior was not exercised".to_owned(),
            );
        }
        let missing_items = (1..=10)
            .filter(|item| !checklist_items.contains(item))
            .collect::<Vec<_>>();
        if !missing_items.is_empty() {
            audit.failures.push(format!(
                "missing operator checklist markers: {missing_items:?}; markers prove only that each item was visited, not that its visual result passed"
            ));
        }
    }
    Ok(audit)
}

fn validate_cursor_range(
    text: &str,
    payload: &Value,
    failures: &mut Vec<String>,
    line_number: usize,
) {
    let begin = payload.get("cursor_begin").and_then(Value::as_u64);
    let end = payload.get("cursor_end").and_then(Value::as_u64);
    if let (Some(begin), Some(end)) = (begin, end) {
        let begin = begin as usize;
        let end = end as usize;
        if begin > end
            || end > text.len()
            || !text.is_char_boundary(begin)
            || !text.is_char_boundary(end)
        {
            failures.push(format!(
                "line {line_number} has invalid byte cursor range {begin}..{end} for {text:?}"
            ));
        }
    }
}

pub fn load_width_cases() -> Result<Vec<WidthCase>> {
    serde_json::from_str(include_str!("../../../corpus/cjk-width-cases.json"))
        .context("parse cjk-width-cases.json")
}

/// Candidate grapheme policy used by this spike for comparison. This is not bt-term's authority.
pub fn candidate_grapheme_cells(text: &str) -> usize {
    UnicodeSegmentation::graphemes(text, true)
        .map(UnicodeWidthStr::width)
        .sum()
}

/// Alternative candidate where East Asian Ambiguous graphemes are wide.
pub fn candidate_grapheme_cells_cjk(text: &str) -> usize {
    UnicodeSegmentation::graphemes(text, true)
        .map(UnicodeWidthStr::width_cjk)
        .sum()
}

pub fn run_shape_audit() -> Result<ShapeAuditReport> {
    let init_started = Instant::now();
    let mut font_system = FontSystem::new();
    let font_initialization_micros = init_started.elapsed().as_micros();
    let mut cases = Vec::new();
    for case in load_width_cases()? {
        cases.push(shape_case(&mut font_system, case)?);
    }
    Ok(ShapeAuditReport {
        schema: "bt-cjk-shape-audit/v1",
        cosmic_text_version: "0.19.0",
        font_initialization_micros,
        cell_width_px: CELL_WIDTH_PX,
        font_size_px: FONT_SIZE_PX,
        line_height_px: LINE_HEIGHT_PX,
        cases,
    })
}

fn shape_case(font_system: &mut FontSystem, case: WidthCase) -> Result<ShapeCaseAudit> {
    let started = Instant::now();
    let mut buffer = Buffer::new(font_system, Metrics::new(FONT_SIZE_PX, LINE_HEIGHT_PX));
    buffer.set_wrap(Wrap::None);
    buffer.set_size(None, None);
    buffer.set_text(&case.text, &Attrs::new(), Shaping::Advanced, None);
    let lines = buffer
        .line_layout(font_system, 0)
        .with_context(|| format!("no layout for {}", case.id))?;
    let line = lines
        .first()
        .with_context(|| format!("empty layout for {}", case.id))?;
    let natural_line_width_px = line.w;
    let glyphs = line.glyphs.clone();
    let shape_micros = started.elapsed().as_micros();

    let mut ranges = UnicodeSegmentation::grapheme_indices(case.text.as_str(), true)
        .map(|(start, cluster)| (start, start + cluster.len(), cluster))
        .collect::<Vec<_>>();
    if ranges.is_empty() && !case.text.is_empty() {
        bail!("{} has text but no grapheme clusters", case.id);
    }

    let mut slot_start_px = 0.0;
    let mut clusters = Vec::with_capacity(ranges.len());
    for (byte_start, byte_end, cluster) in ranges.drain(..) {
        let candidate_cells = UnicodeWidthStr::width(cluster);
        let slot_width_px = candidate_cells as f32 * CELL_WIDTH_PX;
        let overlapping = glyphs
            .iter()
            .filter(|glyph| glyph.start < byte_end && glyph.end > byte_start)
            .collect::<Vec<_>>();
        let natural_start_px = overlapping
            .iter()
            .map(|glyph| glyph.x)
            .reduce(f32::min)
            .unwrap_or(0.0);
        let natural_end_px = overlapping
            .iter()
            .map(|glyph| glyph.x + glyph.w)
            .reduce(f32::max)
            .unwrap_or(natural_start_px);
        let natural_advance_px = (natural_end_px - natural_start_px).max(0.0);
        let clipped = natural_advance_px > slot_width_px;
        let constrained_offset_px = if clipped {
            slot_start_px - natural_start_px
        } else {
            slot_start_px + (slot_width_px - natural_advance_px) / 2.0 - natural_start_px
        };
        clusters.push(ClusterAudit {
            text: cluster.to_owned(),
            byte_start,
            byte_end,
            candidate_cells,
            slot_start_px,
            candidate_slot_width_px: slot_width_px,
            natural_start_px,
            natural_advance_px,
            constrained_offset_px,
            clipped,
            glyph_count: overlapping.len(),
            missing_glyphs: overlapping
                .iter()
                .filter(|glyph| glyph.glyph_id == 0)
                .count(),
        });
        slot_start_px += slot_width_px;
    }
    let candidate_cells = candidate_grapheme_cells(&case.text);
    Ok(ShapeCaseAudit {
        id: case.id,
        text: case.text,
        class: case.class,
        expected_cells: case.expected_cells,
        expected_cells_source: case.expected_cells_source,
        expected_cells_bt_term: case.expected_cells_bt_term,
        expected_cells_bt_term_source: case.expected_cells_bt_term_source,
        candidate_cells,
        natural_line_width_px,
        candidate_slot_width_px: candidate_cells as f32 * CELL_WIDTH_PX,
        shape_micros,
        clusters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::{
        Term, event::VoidListener, grid::Dimensions, term::Config, vte::ansi::Handler,
    };
    use serde_json::json;
    use std::{io::Write, time::SystemTime};

    struct OracleSize;

    impl Dimensions for OracleSize {
        fn total_lines(&self) -> usize {
            2
        }

        fn screen_lines(&self) -> usize {
            2
        }

        fn columns(&self) -> usize {
            128
        }
    }

    fn bt_term_cells(text: &str) -> usize {
        let mut term = Term::new(
            Config {
                scrolling_history: 0,
                ..Config::default()
            },
            &OracleSize,
            VoidListener,
        );
        for character in text.chars() {
            Handler::input(&mut term, character);
        }
        assert!(!term.grid().cursor.input_needs_wrap);
        term.grid().cursor.point.column.0
    }

    fn temp_log(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bt-ime-{label}-{}-{nonce}.jsonl",
            std::process::id()
        ))
    }

    #[test]
    fn explicit_width_corpus_catches_each_candidate_policy_case() {
        let cases = load_width_cases().unwrap();
        let mut ids = BTreeSet::new();
        for case in cases {
            assert!(ids.insert(case.id.clone()), "duplicate id {}", case.id);
            assert!(!case.expected_cells_source.trim().is_empty(), "{}", case.id);
            assert!(
                !case.expected_cells_bt_term_source.trim().is_empty(),
                "{}",
                case.id
            );
            assert_eq!(
                candidate_grapheme_cells(&case.text),
                case.expected_cells,
                "candidate grapheme policy changed for {} ({:?}); check the cited UAX/UTS/product-decision source rather than copying the implementation",
                case.id,
                case.text
            );
        }
    }

    #[test]
    fn vendored_term_input_matches_only_fifteen_candidate_cases() {
        let cases = load_width_cases().unwrap();
        let mut matches = 0;
        let mut mismatches = Vec::new();
        for case in cases {
            let actual = bt_term_cells(&case.text);
            assert_eq!(actual, case.expected_cells_bt_term, "{}", case.id);
            if actual == case.expected_cells {
                matches += 1;
            } else {
                mismatches.push((case.id, case.expected_cells, actual));
            }
        }
        assert_eq!(matches, 15);
        assert_eq!(
            mismatches,
            vec![
                ("heart-emoji-vs16".to_owned(), 2, 1),
                ("keycap".to_owned(), 2, 1),
                ("skin-tone".to_owned(), 2, 4),
                ("woman-technologist".to_owned(), 2, 4),
                ("family-zwj".to_owned(), 2, 8),
                ("rainbow-flag".to_owned(), 2, 3),
                ("wt-900-pencil-emoji".to_owned(), 4, 3),
            ]
        );
    }

    #[test]
    fn ambiguous_width_has_unresolved_narrow_and_cjk_wide_results() {
        let text = "A☆中│Ｂ";
        assert_eq!(candidate_grapheme_cells(text), 7);
        assert_eq!(candidate_grapheme_cells_cjk(text), 9);
    }

    #[test]
    fn constrained_slots_are_driven_only_by_candidate_cells() {
        let report = run_shape_audit().unwrap();
        for case in report.cases {
            assert_eq!(case.candidate_cells, case.expected_cells, "{}", case.id);
            let slot_sum = case
                .clusters
                .iter()
                .map(|cluster| cluster.candidate_slot_width_px)
                .sum::<f32>();
            assert_eq!(slot_sum, case.candidate_slot_width_px, "{}", case.id);
            for cluster in case.clusters {
                assert_eq!(
                    cluster.candidate_slot_width_px,
                    cluster.candidate_cells as f32 * CELL_WIDTH_PX,
                    "{} {:?}",
                    case.id,
                    cluster.text
                );
            }
        }
    }

    #[test]
    fn log_audit_rejects_a_green_but_empty_probe() {
        let path = temp_log("empty");
        let mut file = File::create(&path).unwrap();
        let record = LogRecord {
            schema: LOG_SCHEMA.to_owned(),
            sequence: 0,
            elapsed_micros: 0,
            ime_name: "fault-injection".to_owned(),
            event: "boot".to_owned(),
            payload: json!({}),
        };
        serde_json::to_writer(&mut file, &record).unwrap();
        writeln!(file).unwrap();
        drop(file);
        let audit = audit_log(&path, true).unwrap();
        assert!(!audit.failures.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn strict_log_rejects_minimum_ime_evidence_without_all_ten_markers() {
        let path = temp_log("partial-checklist");
        let mut logger = JsonlLogger::create(&path, "Totally Real Pinyin 9.9".to_owned()).unwrap();
        logger.emit("boot", json!({})).unwrap();
        logger
            .emit("set_ime_cursor_area", json!({"area": {"x": 1, "y": 1}}))
            .unwrap();
        logger.emit("frame", json!({})).unwrap();
        logger.emit("ime_enabled", json!({})).unwrap();
        logger
            .emit(
                "ime_preedit",
                json!({"text": "zhong", "cursor_begin": 5, "cursor_end": 5}),
            )
            .unwrap();
        logger
            .emit("set_ime_cursor_area", json!({"area": {"x": 2, "y": 1}}))
            .unwrap();
        logger.emit("ime_commit", json!({"text": "中"})).unwrap();
        logger.emit("checklist_item", json!({"item": 1})).unwrap();
        logger.emit("checklist_item", json!({"item": 6})).unwrap();
        logger.emit("shutdown", json!({})).unwrap();
        drop(logger);

        let audit = audit_log(&path, true).unwrap();
        assert_eq!(audit.checklist_items, vec![1, 6]);
        assert_eq!(audit.failures.len(), 1, "{:?}", audit.failures);
        assert!(audit.failures[0].contains("[2, 3, 4, 5, 7, 8, 9, 10]"));
        std::fs::remove_file(path).unwrap();
    }
}
