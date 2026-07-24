use std::{
    env,
    error::Error,
    fs, io,
    num::NonZeroU32,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use bt_math::MathEngine;
use bt_term::{
    DualPlaneSession, FormulaFlashOracle, FormulaFrameState, LIVE_MATH_STABLE_INTERVAL,
    SessionMathTask, render_detection_task, render_live_detection_task,
};
use bt_viewport::{ViewportFrame, ViewportProjection};

const FOREGROUND_RGB: [u8; 3] = [0xd8, 0xdc, 0xe8];

struct ReplayChunk<'a> {
    elapsed: Duration,
    bytes: &'a [u8],
    /// A `# RESIZE columns rows elapsed_us` marker recorded before this chunk: the new dimensions
    /// take effect at this point of the replay, exactly as they did in the live session.
    resize_before: Option<(NonZeroU32, NonZeroU32)>,
}

struct HeadlessOracle {
    session: DualPlaneSession,
    projection: ViewportProjection,
    engine: MathEngine,
    flash_oracle: FormulaFlashOracle,
    frame_sequence: usize,
}

impl HeadlessOracle {
    fn new(columns: NonZeroU32, rows: NonZeroU32) -> Self {
        let session = DualPlaneSession::new(columns, rows);
        let projection = session.new_projection(session.layout_key());
        Self {
            session,
            projection,
            engine: MathEngine::new(),
            flash_oracle: FormulaFlashOracle::default(),
            frame_sequence: 0,
        }
    }

    fn advance_before(
        &mut self,
        observed_at: Instant,
        elapsed: Duration,
    ) -> Result<(), Box<dyn Error>> {
        // Mirror the app's timer: a resize epoch only releases live stability once the transaction
        // quiesces, so a replay that never drives this would suppress re-detection forever.
        let _ = self.session.finish_resize_if_quiescent(observed_at);
        self.session.advance_live_stability(observed_at);
        if self.complete_pending_math() {
            self.publish("math-ready", elapsed)?;
        }
        Ok(())
    }

    fn feed(
        &mut self,
        chunk: &[u8],
        observed_at: Instant,
        elapsed: Duration,
    ) -> Result<(), Box<dyn Error>> {
        self.session.feed_at(chunk, observed_at)?;
        self.publish("pty", elapsed)?;
        if self.complete_pending_math() {
            self.publish("math-ready", elapsed)?;
        }
        Ok(())
    }

    fn complete_pending_math(&mut self) -> bool {
        let mut changed = false;
        while let Some(task) = self.session.take_math_worker_task() {
            changed |= match task {
                SessionMathTask::Frozen(mut task) => {
                    let result = render_detection_task(&self.engine, &mut task, FOREGROUND_RGB);
                    self.session.complete_worker_result(task, result)
                }
                SessionMathTask::Live(mut task) => {
                    let result =
                        render_live_detection_task(&self.engine, &mut task, FOREGROUND_RGB);
                    self.session.complete_live_worker_result(task, result)
                }
            };
        }
        changed
    }

    fn publish(&mut self, event: &str, elapsed: Duration) -> Result<(), Box<dyn Error>> {
        self.session.refresh_projection(&mut self.projection);
        let frame = self.session.viewport_frame(&mut self.projection)?;
        let (state, rendered_sources, source_rows, occluded_sources) = {
            let observation = self.flash_oracle.observe(&frame);
            (
                observation.state,
                observation.rendered_sources.clone(),
                observation.source_rows.clone(),
                observation.occluded_sources.clone(),
            )
        };
        let flash_detected = self.flash_oracle.flash_detected();
        println!(
            "frame={} elapsed_us={} event={event} state={:?} rendered={:?} source_rows={:?} occluded={:?} flash={} detections={} invalidations={}",
            self.frame_sequence,
            elapsed.as_micros(),
            state,
            rendered_sources,
            source_rows,
            occluded_sources,
            flash_detected,
            self.session.live_detection_count(),
            self.session.live_invalidation_count(),
        );
        let dump = env::var_os("BT_PROBE_VERBOSE").is_some() && state == FormulaFrameState::Mixed;
        let geometry = env::var_os("BT_PROBE_GEOMETRY").is_some()
            && frame
                .math_blocks
                .iter()
                .any(|block| block.display == bt_viewport::MathBlockDisplay::Rendered);
        if dump {
            self.dump_frame(&frame);
        }
        if geometry {
            self.dump_geometry(&frame);
        }
        self.frame_sequence = self.frame_sequence.saturating_add(1);
        Ok(())
    }

    /// Reconstruct the complete scrollback exactly as a user reviewing it would see it: jump to
    /// the top of the projected document, then walk downward one viewport page at a time,
    /// emitting each visible row once. Overlapping pages are de-duplicated by absolute offset.
    fn document_dump(&mut self) -> Result<(), Box<dyn Error>> {
        self.projection.scroll_to_top();
        self.session.refresh_projection(&mut self.projection);
        let mut emitted_offset = i64::MIN;
        loop {
            // Emulate the app's frame loop: reviewing a page schedules its frozen math artifacts,
            // the worker completes them, and the next frame shows the rendered state.
            let frame = self.session.viewport_frame(&mut self.projection)?;
            self.session.schedule_visible_artifacts(&frame);
            self.complete_pending_math();
            self.session.refresh_projection(&mut self.projection);
            let frame = self.session.viewport_frame(&mut self.projection)?;
            let rendered_history = frame
                .math_blocks
                .iter()
                .filter(|block| {
                    block.display == bt_viewport::MathBlockDisplay::Rendered
                        && matches!(block.anchor, bt_viewport::MathBlockAnchor::History { .. })
                })
                .count();
            let failures = frame.math_failures.len();
            if rendered_history != 0 || failures != 0 {
                eprintln!(
                    "PAGE offset={} rendered_history={rendered_history} failures={failures}",
                    self.projection.scroll_offset_rows(),
                );
                for failure in &frame.math_failures {
                    eprintln!("FAIL {:?}", failure);
                }
            }
            let offset = self.projection.scroll_offset_rows() as i64;
            let rows = frame.rows.get() as i64;
            let columns = frame.columns.get() as usize;
            // Absolute document row of this frame's first visible row grows as offset shrinks.
            let frame_start = -offset;
            for (row, cells) in frame.cells.chunks(columns).enumerate() {
                let absolute = frame_start + row as i64;
                if absolute < emitted_offset {
                    continue;
                }
                let text = cells
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>();
                eprintln!("DOC[{absolute}] |{}", text.trim_end());
                emitted_offset = absolute + 1;
            }
            if offset == 0 {
                break;
            }
            let step = i32::try_from(rows.min(offset)).unwrap_or(i32::MAX);
            self.projection.scroll_by_rows(-step);
            self.session.refresh_projection(&mut self.projection);
        }
        Ok(())
    }

    fn scroll_top_probe(&mut self) -> Result<(), Box<dyn Error>> {
        // From the resting (bottom) view, scroll up one row at a time and report the highest
        // rendered artifact top after each step. If the top never reaches >= 0 before the offset
        // stops growing, the upward allowance cannot bring a tall block's top fully into the pane.
        let mut last_offset = usize::MAX;
        for step in 0..60 {
            self.projection.scroll_by_rows(1);
            self.session.refresh_projection(&mut self.projection);
            let frame = self.session.viewport_frame(&mut self.projection)?;
            let min_top = frame
                .math_blocks
                .iter()
                .filter(|block| block.display == bt_viewport::MathBlockDisplay::Rendered)
                .map(|block| {
                    block
                        .top_subpixels
                        .saturating_add(block.content_offset_subpixels)
                })
                .min();
            let offset = self.projection.scroll_offset_rows();
            let (total, hist_off, live_allow, live_used, unread) =
                self.projection.debug_scroll_extent();
            eprintln!(
                "SCROLLUP step={step} offset_rows={offset} total={total} hist_off={hist_off} live_allow={live_allow} live_used={live_used} unread={unread} min_artifact_top={:?} rendered={}",
                min_top,
                frame
                    .math_blocks
                    .iter()
                    .filter(|b| b.display == bt_viewport::MathBlockDisplay::Rendered)
                    .count(),
            );
            if offset == last_offset {
                eprintln!("SCROLLUP capped at offset_rows={offset} after {step} steps");
                self.dump_geometry(&frame);
                break;
            }
            last_offset = offset;
        }
        Ok(())
    }

    fn dump_geometry(&self, frame: &ViewportFrame) {
        for block in &frame.math_blocks {
            if block.display != bt_viewport::MathBlockDisplay::Rendered {
                continue;
            }
            let (band_start, band_end, occurrence) = match block.anchor {
                bt_viewport::MathBlockAnchor::Live {
                    band_start_row,
                    band_end_row,
                    ..
                } => (
                    band_start_row as i64,
                    band_end_row as i64,
                    block.live_occurrence_id.map(|id| id.0),
                ),
                _ => (-1, -1, None),
            };
            let source_head = block
                .source
                .replace('\n', " ")
                .chars()
                .take(24)
                .collect::<String>();
            eprintln!(
                "GEOM frame={} occ_id={:?} band={}..={} occ_rows={} top_sub={} content_off={} clip_h={} art_h={} pad={} src=\"{}\"",
                self.frame_sequence,
                occurrence,
                band_start,
                band_end,
                block.occluded_source_rows,
                block.top_subpixels,
                block.content_offset_subpixels,
                block.clip_height_subpixels,
                block.artifact.height_subpixels,
                block.artifact.vertical_padding_subpixels,
                source_head,
            );
        }
    }

    fn dump_frame(&self, frame: &ViewportFrame) {
        eprintln!("### FRAME {} (Mixed)", self.frame_sequence);
        for (index, block) in frame.math_blocks.iter().enumerate() {
            let (band_start, band_end, occurrence) = match block.anchor {
                bt_viewport::MathBlockAnchor::Live {
                    band_start_row,
                    band_end_row,
                    ..
                } => (
                    band_start_row as i64,
                    band_end_row as i64,
                    block.live_occurrence_id.map(|id| id.0),
                ),
                _ => (-1, -1, None),
            };
            let source_head = block
                .source
                .replace('\n', " ")
                .chars()
                .take(40)
                .collect::<String>();
            eprintln!(
                "  block[{index}] display={:?} band={}..={} occluded={} occ_id={:?} src=\"{}\"",
                block.display,
                band_start,
                band_end,
                block.occluded_source_rows,
                occurrence,
                source_head,
            );
        }
        let columns = frame.columns.get() as usize;
        for (row, cells) in frame.cells.chunks(columns).enumerate() {
            let text = cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>();
            let trimmed = text.trim_end();
            if !trimmed.is_empty() {
                eprintln!("  row[{row:>2}] |{trimmed}");
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let input_path = env::var_os("BT_PROBE_INPUT")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "BT_PROBE_INPUT is required"))?;
    let input = fs::read(&input_path)?;
    let chunks_path = env::var_os("BT_PROBE_CHUNKS")
        .map(PathBuf::from)
        .unwrap_or_else(|| append_suffix(&input_path, ".chunks"));
    let chunks = if chunks_path.is_file() {
        parse_chunks(&input, &fs::read_to_string(&chunks_path)?)?
    } else {
        vec![ReplayChunk {
            elapsed: Duration::ZERO,
            bytes: &input,
            resize_before: None,
        }]
    };
    let columns = env_dimension("BT_PROBE_COLUMNS", 120)?;
    let rows = env_dimension("BT_PROBE_ROWS", 40)?;
    let started = Instant::now();
    let mut oracle = HeadlessOracle::new(columns, rows);
    let mut final_elapsed = Duration::ZERO;

    for chunk in chunks {
        final_elapsed = chunk.elapsed;
        let observed_at = started + chunk.elapsed;
        if let Some((columns, rows)) = chunk.resize_before {
            oracle.session.resize_at(columns, rows, observed_at)?;
            // The marker is written when the PTY itself is resized, so both the local resize and
            // the ConPTY acknowledgement happen here, exactly like the app's coalesced flush.
            oracle
                .session
                .mark_pty_resize_requested_at(columns, rows, observed_at);
            oracle.publish("resize", chunk.elapsed)?;
        }
        oracle.advance_before(observed_at, chunk.elapsed)?;
        oracle.feed(chunk.bytes, observed_at, chunk.elapsed)?;
    }
    final_elapsed = final_elapsed.saturating_add(LIVE_MATH_STABLE_INTERVAL);
    oracle.advance_before(started + final_elapsed, final_elapsed)?;

    if env::var_os("BT_PROBE_GEOMETRY").is_some() || env::var_os("BT_PROBE_SCROLLTOP").is_some() {
        let (total, hist_off, live_allow, live_used, unread) =
            oracle.projection.debug_scroll_extent();
        eprintln!(
            "RESTEXTENT total={total} hist_off={hist_off} live_allow={live_allow} live_used={live_used} unread={unread}"
        );
    }

    if env::var_os("BT_PROBE_SCROLLTOP").is_some() {
        oracle.scroll_top_probe()?;
    }

    if env::var_os("BT_PROBE_DOCDUMP").is_some() {
        oracle.document_dump()?;
    }

    if env::var_os("BT_PROBE_FROZEN").is_some() {
        for line in oracle.session.transcript().frozen() {
            eprintln!("FROZEN[{}] |{}", line.id.0, line.text);
            if env::var_os("BT_PROBE_STYLES").is_some() {
                for span in &line.styles {
                    eprintln!("  STYLE {span:?}");
                }
            }
        }
    }

    if oracle.flash_oracle.flash_detected() {
        return Err(io::Error::other(format!(
            "formula repaint flash detected for {:?}",
            oracle.flash_oracle.flashed_sources()
        ))
        .into());
    }
    Ok(())
}

fn parse_chunks<'a>(
    input: &'a [u8],
    manifest: &str,
) -> Result<Vec<ReplayChunk<'a>>, Box<dyn Error>> {
    let mut chunks = Vec::new();
    let mut offset = 0usize;
    let mut expected_sequence = 0u64;
    let mut previous_elapsed = Duration::ZERO;
    let mut pending_resize = None;
    for (line_index, line) in manifest.lines().enumerate() {
        let line = line.trim();
        if let Some(resize) = line.strip_prefix("# RESIZE ") {
            let fields = resize.split_ascii_whitespace().collect::<Vec<_>>();
            let (Some(columns), Some(rows)) = (
                fields.first().and_then(|f| f.parse::<u32>().ok()),
                fields.get(1).and_then(|f| f.parse::<u32>().ok()),
            ) else {
                return Err(invalid_manifest(line_index, "expected: # RESIZE cols rows").into());
            };
            let (Some(columns), Some(rows)) = (NonZeroU32::new(columns), NonZeroU32::new(rows))
            else {
                return Err(
                    invalid_manifest(line_index, "resize dimensions must be non-zero").into(),
                );
            };
            pending_resize = Some((columns, rows));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err(invalid_manifest(line_index, "expected: sequence elapsed_us bytes").into());
        }
        let sequence = fields[0].parse::<u64>()?;
        let elapsed = Duration::from_micros(fields[1].parse::<u64>()?);
        let length = fields[2].parse::<usize>()?;
        if sequence != expected_sequence {
            return Err(invalid_manifest(line_index, "non-contiguous sequence number").into());
        }
        if elapsed < previous_elapsed {
            return Err(invalid_manifest(line_index, "arrival time moved backwards").into());
        }
        let end = offset.checked_add(length).ok_or_else(|| {
            invalid_manifest(line_index, "chunk length overflowed the input offset")
        })?;
        let bytes = input
            .get(offset..end)
            .ok_or_else(|| invalid_manifest(line_index, "chunk lengths exceed BT_PROBE_INPUT"))?;
        chunks.push(ReplayChunk {
            elapsed,
            bytes,
            resize_before: pending_resize.take(),
        });
        offset = end;
        expected_sequence = expected_sequence.saturating_add(1);
        previous_elapsed = elapsed;
    }
    if offset != input.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "chunk manifest accounts for {offset} of {} BT_PROBE_INPUT bytes",
                input.len()
            ),
        )
        .into());
    }
    Ok(chunks)
}

fn invalid_manifest(line_index: usize, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid chunk manifest line {}: {message}", line_index + 1),
    )
}

fn env_dimension(name: &str, default: u32) -> Result<NonZeroU32, Box<dyn Error>> {
    let value = env::var(name)
        .ok()
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(default);
    NonZeroU32::new(value)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be non-zero"),
            )
        })
        .map_err(Into::into)
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut result = path.as_os_str().to_os_string();
    result.push(suffix);
    PathBuf::from(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_manifest_slices_the_exact_raw_stream() {
        let input = b"abcdef";
        let chunks = parse_chunks(
            input,
            "# BT_PTY_DUMP_CHUNKS_V1 sequence elapsed_us bytes\n0 10 2\n1 20 4\n",
        )
        .unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].bytes, b"ab");
        assert_eq!(chunks[1].bytes, b"cdef");
        assert_eq!(chunks[1].elapsed, Duration::from_micros(20));
    }
}
