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
    pub class: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClusterAudit {
    pub text: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub terminal_cells: usize,
    pub slot_start_px: f32,
    pub slot_width_px: f32,
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
    pub policy_cells: usize,
    pub natural_line_width_px: f32,
    pub authoritative_width_px: f32,
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
    pub failures: Vec<String>,
}

pub fn audit_log(path: &Path, strict_ime: bool) -> Result<LogAudit> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut audit = LogAudit::default();
    let mut areas = BTreeSet::new();
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
            _ => {}
        }
    }
    audit.distinct_candidate_areas = areas.len();
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

pub fn terminal_policy_cells(text: &str) -> usize {
    UnicodeSegmentation::graphemes(text, true)
        .map(UnicodeWidthStr::width)
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
        let terminal_cells = UnicodeWidthStr::width(cluster);
        let slot_width_px = terminal_cells as f32 * CELL_WIDTH_PX;
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
            terminal_cells,
            slot_start_px,
            slot_width_px,
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
    let policy_cells = terminal_policy_cells(&case.text);
    Ok(ShapeCaseAudit {
        id: case.id,
        text: case.text,
        class: case.class,
        expected_cells: case.expected_cells,
        policy_cells,
        natural_line_width_px,
        authoritative_width_px: policy_cells as f32 * CELL_WIDTH_PX,
        shape_micros,
        clusters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn explicit_width_corpus_catches_each_policy_case() {
        let cases = load_width_cases().unwrap();
        let mut ids = BTreeSet::new();
        for case in cases {
            assert!(ids.insert(case.id.clone()), "duplicate id {}", case.id);
            assert_eq!(
                terminal_policy_cells(&case.text),
                case.expected_cells,
                "terminal width policy changed for {} ({:?})",
                case.id,
                case.text
            );
        }
    }

    #[test]
    fn constrained_slots_are_driven_only_by_terminal_cells() {
        let report = run_shape_audit().unwrap();
        for case in report.cases {
            assert_eq!(case.policy_cells, case.expected_cells, "{}", case.id);
            let slot_sum = case
                .clusters
                .iter()
                .map(|cluster| cluster.slot_width_px)
                .sum::<f32>();
            assert_eq!(slot_sum, case.authoritative_width_px, "{}", case.id);
            for cluster in case.clusters {
                assert_eq!(
                    cluster.slot_width_px,
                    cluster.terminal_cells as f32 * CELL_WIDTH_PX,
                    "{} {:?}",
                    case.id,
                    cluster.text
                );
            }
        }
    }

    #[test]
    fn log_audit_rejects_a_green_but_empty_probe() {
        let path = std::env::temp_dir().join(format!(
            "bt-ime-empty-{}-{}.jsonl",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
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
}
