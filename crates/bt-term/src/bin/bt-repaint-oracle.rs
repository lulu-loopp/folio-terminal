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

/// A math task taken from the session, awaiting its deferred completion. Models the off-thread
/// bt-math worker: the render/apply lands `math_latency` after the task was dispatched, never
/// synchronously in the same feed (see `HeadlessOracle::math_latency`).
struct DeferredMath {
    ready_at: Instant,
    task: SessionMathTask,
}

struct HeadlessOracle {
    session: DualPlaneSession,
    projection: ViewportProjection,
    engine: MathEngine,
    flash_oracle: FormulaFlashOracle,
    frame_sequence: usize,
    started: Instant,
    /// When `Some(latency)`, math completes `latency` after the task is dispatched instead of
    /// synchronously inside the feed. This restores the real machine's async gap: a resize (or an
    /// in-stream reprint) demotes a raster to stale and queues a relayout that lands one latency
    /// later, so the protection-lift-vs-fresh-landing ordering the flash needs actually forms.
    /// `None` keeps the historical synchronous behaviour (byte-identical regression runs).
    math_latency: Option<Duration>,
    /// Dispatched-but-not-yet-completed math tasks, ordered by arrival. Faithful to the real
    /// worker: currency is re-checked at apply time, so a task whose generation/revision/layout
    /// was bumped mid-flight is rejected exactly as it would be on the main thread.
    deferred: Vec<DeferredMath>,
}

impl HeadlessOracle {
    fn new(columns: NonZeroU32, rows: NonZeroU32, started: Instant) -> Self {
        let session = DualPlaneSession::new(columns, rows);
        let projection = session.new_projection(session.layout_key());
        let math_latency = env::var("BT_PROBE_MATH_LATENCY_US")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|micros| *micros > 0)
            .map(Duration::from_micros);
        Self {
            session,
            projection,
            engine: MathEngine::new(),
            flash_oracle: FormulaFlashOracle::default(),
            frame_sequence: 0,
            started,
            math_latency,
            deferred: Vec::new(),
        }
    }

    fn advance_before(
        &mut self,
        observed_at: Instant,
        elapsed: Duration,
    ) -> Result<(), Box<dyn Error>> {
        if self.math_latency.is_some() {
            // Deadline-driven ticks between chunks (mirroring winit `WaitUntil`) already fired every
            // internal deadline strictly before `observed_at`; run one more at exactly this instant.
            self.tick(observed_at, elapsed)?;
            return Ok(());
        }
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
        if self.math_latency.is_some() {
            // Off-thread model: the reprint's damage has already dropped/held decorations; new
            // detection tasks are only dispatched here and land one latency later, never now.
            self.dispatch_delayed(observed_at);
        } else if self.complete_pending_math() {
            self.publish("math-ready", elapsed)?;
        }
        Ok(())
    }

    /// Take every task the session currently has queued and stamp it with a completion deadline one
    /// latency in the future. The task leaves the session's queue exactly as the real worker takes
    /// it; only its result is deferred.
    fn dispatch_delayed(&mut self, now: Instant) {
        let latency = self.math_latency.expect("delayed dispatch without latency");
        while let Some(task) = self.session.take_math_worker_task() {
            self.deferred.push(DeferredMath {
                ready_at: now + latency,
                task,
            });
        }
    }

    /// Apply every deferred task whose deadline has passed, rendering it now and handing the result
    /// back to the session (which re-checks currency and rejects stale results, just like the app).
    fn apply_matured(&mut self, now: Instant) -> bool {
        let mut changed = false;
        let mut index = 0;
        while index < self.deferred.len() {
            if self.deferred[index].ready_at > now {
                index += 1;
                continue;
            }
            let DeferredMath { task, .. } = self.deferred.remove(index);
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

    fn next_internal_deadline(&self) -> Option<Instant> {
        // A synchronized update is deliberately excluded: these captures balance every `2026h` with
        // an in-band `2026l`, so `feed_at` commits each update at its own chunk exactly as recorded.
        // Driving the timeout here would preempt an update whose ESU is only a chunk away, desyncing
        // the replay — the historical (synchronous) oracle never drove it either.
        [
            self.deferred.iter().map(|d| d.ready_at).min(),
            self.session.resize_finish_deadline(),
            self.session.live_stability_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// One event-loop wake at `now`: apply matured completions, close a quiesced resize
    /// transaction, release live stability, and dispatch whatever new detection those unblock —
    /// mirroring the app's `about_to_wait` ordering.
    fn tick(&mut self, now: Instant, elapsed: Duration) -> Result<(), Box<dyn Error>> {
        if self.apply_matured(now) {
            self.publish("math-ready", elapsed)?;
        }
        if self.session.finish_resize_if_quiescent(now)? {
            self.publish("resize-finish", elapsed)?;
        }
        self.session.advance_live_stability(now);
        self.dispatch_delayed(now);
        Ok(())
    }

    /// Advance the replay clock through every internal deadline strictly before `target`, ticking at
    /// each, so the post-quiescence gap physically exists in the replay and a reprint chunk can land
    /// after protection has lifted.
    fn drive_until(&mut self, target: Instant) -> Result<(), Box<dyn Error>> {
        let mut guard = 0_u32;
        while let Some(next) = self.next_internal_deadline().filter(|d| *d < target) {
            let elapsed = next.saturating_duration_since(self.started);
            self.tick(next, elapsed)?;
            guard += 1;
            if guard > 1_000_000 {
                return Err(io::Error::other("delayed replay failed to converge").into());
            }
        }
        Ok(())
    }

    /// Drain all remaining deferred work after the last chunk so the final resting state converges,
    /// exactly as the app eventually settles once output stops.
    fn drain_to_quiescence(&mut self) -> Result<(), Box<dyn Error>> {
        let mut guard = 0_u32;
        while let Some(next) = self.next_internal_deadline() {
            let elapsed = next.saturating_duration_since(self.started);
            self.tick(next, elapsed)?;
            guard += 1;
            if guard > 1_000_000 {
                return Err(io::Error::other("delayed replay failed to drain").into());
            }
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
        let (state, rendered_sources, source_rows, source_plane, occluded_sources) = {
            let observation = self.flash_oracle.observe(&frame);
            (
                observation.state,
                observation.rendered_sources.clone(),
                observation.source_rows.clone(),
                observation.source_plane.clone(),
                observation.occluded_sources.clone(),
            )
        };
        let flash_detected = self.flash_oracle.flash_detected();
        // `source_plane` retains the delimiter-free body rows a multi-line block drops from
        // `source_rows`, so trace_blocks.py can count a split-body revert as a real R->S flip.
        println!(
            "frame={} elapsed_us={} event={event} state={:?} rendered={:?} source_rows={:?} occluded={:?} flash={} detections={} invalidations={} source_plane={:?}",
            self.frame_sequence,
            elapsed.as_micros(),
            state,
            rendered_sources,
            source_rows,
            occluded_sources,
            flash_detected,
            self.session.live_detection_count(),
            self.session.live_invalidation_count(),
            source_plane,
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
    let mut oracle = HeadlessOracle::new(columns, rows, started);
    let delayed = oracle.math_latency.is_some();
    let mut final_elapsed = Duration::ZERO;

    for chunk in chunks {
        final_elapsed = chunk.elapsed;
        let observed_at = started + chunk.elapsed;
        if delayed {
            // Fire every internal deadline that falls before this chunk arrives, so a queued
            // relayout can land and a resize can quiesce before the next PTY bytes.
            oracle.drive_until(observed_at)?;
        }
        if let Some((columns, rows)) = chunk.resize_before {
            oracle.session.resize_at(columns, rows, observed_at)?;
            // The marker is written when the PTY itself is resized, so both the local resize and
            // the ConPTY acknowledgement happen here, exactly like the app's coalesced flush.
            oracle
                .session
                .mark_pty_resize_requested_at(columns, rows, observed_at);
            oracle.publish("resize", chunk.elapsed)?;
            if delayed {
                // The resize demotes rasters to stale and queues relayouts; dispatch them so they
                // land one latency later rather than instantly.
                oracle.dispatch_delayed(observed_at);
            }
        }
        oracle.advance_before(observed_at, chunk.elapsed)?;
        oracle.feed(chunk.bytes, observed_at, chunk.elapsed)?;
    }
    if delayed {
        oracle.drain_to_quiescence()?;
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

    if env::var_os("BT_PROBE_STAGED").is_some() {
        for staged in oracle.session.transcript().staged_rows() {
            let text = staged
                .row
                .cells
                .iter()
                .filter(|cell| !cell.wide_spacer)
                .map(|cell| cell.text.as_str())
                .collect::<String>();
            eprintln!(
                "STAGED[{}] continues={} |{}",
                staged.id.0,
                staged.row.continues,
                text.trim_end()
            );
        }
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
