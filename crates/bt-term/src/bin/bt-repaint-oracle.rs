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
    /// Peak document-level detection gap seen across the run: on-screen blocks provable by a clean
    /// grid-only re-scan but absent from the full history+grid detection. See the summary line.
    max_isolation_gap: usize,
    /// Batch ⑥ detector-containment. Peak count of UNANNOTATED ownership orphans seen across the
    /// run, whether the clipped-open topology ever appeared, and the recording's source-integrity
    /// annotations (known upstream byte damage, precisely isolated so it does not red the gate).
    /// Only tracked per-frame when `BT_PROBE_OWNERSHIP` is set; the final-state verdict is always
    /// computed. See `OWNERSHIP_LEDGER`.
    max_orphans: usize,
    ever_clipped_open: bool,
    annotations: Vec<bt_detect::SourceIntegrityAnnotation>,
    /// Batch ③ stale-hold honesty. Peak count of UNANNOTATED `HeldUnbacked` records seen across the
    /// run: a formula still displayed (live decoration / stale artifact / off-band hold) whose exact
    /// source the current detection scan no longer Owns. A transient spike is legitimate (reprint /
    /// resize / stream-in); a nonzero FINAL value is the hold-masks-dead-detection strand. Tracked
    /// every frame (the field is emitted per frame); the final-state verdict drives the opt-in gate.
    max_held_unbacked: usize,
    /// Zoom/reflow stale-window band-alignment gate. Peak per-frame count of primary Rendered live
    /// blocks whose clip height falls short of their artifact height WITHOUT a genuine-clip context
    /// (bottom-edge run-off or occlusion) — the half-band fragment a phantom top clip leaves in the
    /// stale preview window. Frame transient == wall-clock persistent during idle, so any nonzero
    /// value is a defect the user sees; the gate reds on the peak, not just the final state.
    max_clip_misalign: usize,
    /// A human-readable exemplar of the worst clip-misalignment placement seen, for the report.
    clip_misalign_worst: Option<String>,
    /// Peak number of non-blank terminal presentation cells left underneath a rendered math band
    /// (or inside one of its source-proven occlusion ranges). Text is not the only terminal ink:
    /// underline/inverse/background state on an empty cell still draws pixels behind the raster.
    max_occlusion_residue_cells: usize,
    /// A human-readable exemplar of the worst source-occlusion residue frame.
    occlusion_residue_worst: Option<String>,
    /// Peak count of live band rows outside a rendered occurrence's exact source rows. Such rows
    /// are separators or TUI chrome, not presentation-owned formula source.
    max_borrowed_band_rows: usize,
    /// Peak count of pairs of rendered live bands sharing a terminal row.
    max_overlapping_live_bands: usize,
    /// Visible display-source rows in the most recently published frame.
    final_visible_source_rows: usize,
    /// A human-readable exemplar for either layout-ownership violation.
    layout_worst: Option<String>,
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
            max_isolation_gap: 0,
            max_orphans: 0,
            ever_clipped_open: false,
            annotations: Vec::new(),
            max_held_unbacked: 0,
            max_clip_misalign: 0,
            clip_misalign_worst: None,
            max_occlusion_residue_cells: 0,
            occlusion_residue_worst: None,
            max_borrowed_band_rows: 0,
            max_overlapping_live_bands: 0,
            final_visible_source_rows: 0,
            layout_worst: None,
        }
    }

    /// Frame-level red gate for the zoom/reflow stale-window residue. A primary Rendered live block
    /// whose clip height is short of its artifact height is only legitimate when a genuine clip
    /// context is present: a bottom-edge run-off (the band ends on the last live row) or occlusion.
    /// Every other short clip is a phantom top clip — the stale identity out-counting the reflowed
    /// occurrence's rows — which the user sees as a half-band / stray fragment. Boundary-split
    /// bridges (`frozen_prefix_rows > 0`) legitimately carry a combined height and are exempt.
    fn audit_clip_alignment(&mut self, frame: &ViewportFrame) {
        let last_live_row = frame.rows.get().saturating_sub(1);
        let mut violations = 0usize;
        for block in &frame.math_blocks {
            if block.display != bt_viewport::MathBlockDisplay::Rendered {
                continue;
            }
            let bt_viewport::MathBlockAnchor::Live { band_end_row, .. } = block.anchor else {
                continue;
            };
            if block.frozen_prefix_rows > 0 {
                continue;
            }
            if block.clip_height_subpixels >= block.artifact.height_subpixels {
                continue;
            }
            let genuine_bottom_clip =
                block.clipped_bottom_rows > 0 && band_end_row == last_live_row;
            let occluded =
                block.occluded_source_rows > 0 || !block.occluded_visible_rows.is_empty();
            if genuine_bottom_clip || occluded {
                continue;
            }
            violations += 1;
            if self.clip_misalign_worst.is_none() {
                let source_head = block
                    .source
                    .replace('\n', " ")
                    .chars()
                    .take(28)
                    .collect::<String>();
                self.clip_misalign_worst = Some(format!(
                    "frame={} band={:?}..={band_end_row} clip_h={} art_h={} clip_top={} clip_bot={} occ={} scale={} src=\"{source_head}\"",
                    self.frame_sequence,
                    match block.anchor {
                        bt_viewport::MathBlockAnchor::Live { band_start_row, .. } => band_start_row,
                        _ => 0,
                    },
                    block.clip_height_subpixels,
                    block.artifact.height_subpixels,
                    block.clipped_top_rows,
                    block.clipped_bottom_rows,
                    block.occluded_source_rows,
                    block.artifact.render_scale_milli,
                ));
            }
        }
        self.max_clip_misalign = self.max_clip_misalign.max(violations);
    }

    /// Frame-level red gate for terminal ink surviving underneath a rendered formula. Formula
    /// source suppression must clear the complete `CapturedCell` presentation in the owned band,
    /// and the exact proven cells in an occluded row. Otherwise textless SGR state (most visibly
    /// UNDERLINE) is still rendered as a long horizontal line through the transparent math raster.
    fn audit_occlusion_residue(&mut self, frame: &ViewportFrame, elapsed: Duration) {
        let columns = frame.columns.get() as usize;
        let blank = bt_transcript::CapturedCell::default();
        let mut residue = 0usize;
        let mut sources = Vec::new();
        for block in &frame.math_blocks {
            if block.display != bt_viewport::MathBlockDisplay::Rendered {
                continue;
            }
            let bt_viewport::MathBlockAnchor::Live {
                band_start_row,
                band_end_row,
                ..
            } = block.anchor
            else {
                continue;
            };
            let before = residue;
            for (frame_row, mapped) in frame.row_map.iter().enumerate() {
                let Some(live_row) = mapped.live_grid_row else {
                    continue;
                };
                let ranges = if (band_start_row..=band_end_row).contains(&live_row) {
                    vec![(0usize, columns)]
                } else {
                    block
                        .occluded_visible_rows
                        .iter()
                        .find(|(row, _)| *row == live_row)
                        .map(|(_, ranges)| {
                            ranges
                                .iter()
                                .map(|(start, end)| {
                                    ((*start as usize).min(columns), (*end as usize).min(columns))
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let row_start = frame_row.saturating_mul(columns);
                for (start, end) in ranges {
                    if start >= end {
                        continue;
                    }
                    residue += frame.cells[row_start + start..row_start + end]
                        .iter()
                        .filter(|cell| **cell != blank)
                        .count();
                }
            }
            if residue > before {
                sources.push(
                    block
                        .source
                        .replace('\n', " ")
                        .chars()
                        .take(28)
                        .collect::<String>(),
                );
            }
        }
        if residue == 0 {
            return;
        }
        if env::var_os("BT_PROBE_OCCLUSION_AUDIT").is_some() {
            eprintln!(
                "OCCLUSION_RESIDUE frame={} elapsed_us={} cells={residue} sources={sources:?}",
                self.frame_sequence,
                elapsed.as_micros(),
            );
        }
        if residue > self.max_occlusion_residue_cells {
            self.max_occlusion_residue_cells = residue;
            self.occlusion_residue_worst = Some(format!(
                "frame={} elapsed_us={} cells={residue} sources={sources:?}",
                self.frame_sequence,
                elapsed.as_micros(),
            ));
        }
    }

    /// Geometry/ownership red gate for the residue recording. A live raster may expand the pixel
    /// height of its exact source rows, but it must not claim neighbouring blank separators or
    /// textless input chrome, and two blocks must never share one terminal row. Both violations
    /// were directly visible in the historical frame geometry even though formula-state stdout was
    /// otherwise byte-identical across the suspect commits.
    fn audit_live_band_ownership(&mut self, frame: &ViewportFrame) {
        let mut bands = Vec::new();
        let mut borrowed = 0usize;
        for block in &frame.math_blocks {
            if block.display != bt_viewport::MathBlockDisplay::Rendered {
                continue;
            }
            let bt_viewport::MathBlockAnchor::Live {
                screen,
                start,
                end,
                band_start_row,
                band_end_row,
                generation,
            } = block.anchor
            else {
                continue;
            };
            let outside = start
                .row
                .saturating_sub(band_start_row)
                .saturating_add(band_end_row.saturating_sub(end.row));
            borrowed = borrowed.saturating_add(outside as usize);
            if outside != 0 && self.layout_worst.is_none() {
                self.layout_worst = Some(format!(
                    "frame={} source={}..={} band={band_start_row}..={band_end_row} src={:?}",
                    self.frame_sequence,
                    start.row,
                    end.row,
                    block
                        .source
                        .replace('\n', " ")
                        .chars()
                        .take(28)
                        .collect::<String>(),
                ));
            }
            bands.push((screen, generation, band_start_row, band_end_row));
        }
        let mut overlaps = 0usize;
        for (index, left) in bands.iter().enumerate() {
            for right in &bands[index + 1..] {
                if left.0 == right.0 && left.1 == right.1 && left.2 <= right.3 && right.2 <= left.3
                {
                    overlaps = overlaps.saturating_add(1);
                    if self.layout_worst.is_none() {
                        self.layout_worst = Some(format!(
                            "frame={} overlapping_bands={}..={} and {}..={}",
                            self.frame_sequence, left.2, left.3, right.2, right.3,
                        ));
                    }
                }
            }
        }
        self.max_borrowed_band_rows = self.max_borrowed_band_rows.max(borrowed);
        self.max_overlapping_live_bands = self.max_overlapping_live_bands.max(overlaps);
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
        self.final_visible_source_rows = source_rows.len();
        let isolation_gap = self.session.live_detection_isolation_gap();
        self.max_isolation_gap = self.max_isolation_gap.max(isolation_gap);
        // Per-frame detector-containment (batch ⑥). Opt-in: the ledger re-runs the scan, so track it
        // per frame only under BT_PROBE_OWNERSHIP; the final-state verdict is always computed below.
        // Hold-independent — a masked-by-holds strand still surfaces as an orphan here.
        if env::var_os("BT_PROBE_OWNERSHIP").is_some() {
            let verdict = self
                .session
                .live_detection_ownership_ledger()
                .containment(&self.annotations);
            self.max_orphans = self.max_orphans.max(verdict.orphans);
            self.ever_clipped_open |= verdict.clipped_open;
        }
        // Batch ③ stale-hold honesty. Every displayed formula whose exact source the current scan no
        // longer Owns is `HeldUnbacked` — a hold potentially masking dead detection. Annotated
        // known-legitimate long-lived forms are excluded (precise, never blanket). Reported per frame;
        // a transient spike is legitimate, the final-state count drives the opt-in gate below.
        let held_unbacked = self.session.held_unbacked_records();
        let held_unbacked_count = held_unbacked
            .iter()
            .filter(|record| !held_unbacked_is_annotated(record, &self.annotations))
            .count();
        self.max_held_unbacked = self.max_held_unbacked.max(held_unbacked_count);
        // `source_plane` retains the delimiter-free body rows a multi-line block drops from
        // `source_rows`, so trace_blocks.py can count a split-body revert as a real R->S flip.
        // `isolation_gap` is the document-level detection red gate: on-screen blocks provable by a
        // clean grid-only re-scan yet missing from the full detection (a poisoned-history desync the
        // flash oracle cannot see). A healthy frame is 0.
        println!(
            "frame={} elapsed_us={} event={event} state={:?} rendered={:?} source_rows={:?} occluded={:?} flash={} detections={} invalidations={} isolation_gap={isolation_gap} source_plane={:?} held_unbacked={held_unbacked_count}",
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
        self.audit_clip_alignment(&frame);
        self.audit_occlusion_residue(&frame, elapsed);
        self.audit_live_band_ownership(&frame);
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

    /// Faithful review-state render audit (scheduling-loss red gate). Unlike `document_dump`, this
    /// never re-schedules: it walks the settled document top-to-bottom exactly as a reviewer would
    /// see it right now, reading only the decoration state the live replay already produced. A block
    /// whose frozen detection scan was swallowed (e.g. ingested inside a resize suppression window
    /// and never re-issued) therefore surfaces here as raw source rows, where `document_dump`'s
    /// fresh per-page scheduling would silently render it. A healthy settled state is `source_rows=0`.
    fn review_render_audit(&mut self) -> Result<(), Box<dyn Error>> {
        self.projection.scroll_to_top();
        self.session.refresh_projection(&mut self.projection);
        let mut emitted_offset = i64::MIN;
        let mut total_source_rows = 0usize;
        let mut total_rendered_history = 0usize;
        let mut pages = 0usize;
        loop {
            // No schedule_visible_artifacts, no complete_pending_math: the settled decoration state
            // is read as-is, so a stranded (unscheduled) block shows its source instead of a raster.
            let frame = self.session.viewport_frame(&mut self.projection)?;
            let rendered_history = frame
                .math_blocks
                .iter()
                .filter(|block| {
                    block.display == bt_viewport::MathBlockDisplay::Rendered
                        && matches!(block.anchor, bt_viewport::MathBlockAnchor::History { .. })
                })
                .count();
            let offset = self.projection.scroll_offset_rows() as i64;
            let rows = frame.rows.get() as i64;
            let columns = frame.columns.get() as usize;
            let frame_start = -offset;
            let mut page_source_rows = Vec::new();
            for (row, cells) in frame.cells.chunks(columns).enumerate() {
                let absolute = frame_start + row as i64;
                if absolute < emitted_offset {
                    continue;
                }
                emitted_offset = absolute + 1;
                let text = cells
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>();
                let trimmed = text.trim();
                // A settled review page shows rendered rasters over the math span; a raw structural
                // delimiter surviving on a history row means that block was never decorated.
                if trimmed.contains("$$")
                    || trimmed.contains(r"\begin{")
                    || trimmed.contains(r"\end{")
                    || trimmed.contains(r"\[")
                    || trimmed.contains(r"\]")
                {
                    page_source_rows.push((absolute, trimmed.to_owned()));
                }
            }
            if rendered_history != 0 || !page_source_rows.is_empty() {
                eprintln!(
                    "REVIEW_PAGE offset={} rendered_history={rendered_history} source_rows={}",
                    -frame_start,
                    page_source_rows.len(),
                );
                for (absolute, text) in &page_source_rows {
                    eprintln!("  SRC[{absolute}] |{text}");
                }
            }
            total_source_rows += page_source_rows.len();
            total_rendered_history += rendered_history;
            pages += 1;
            if offset == 0 {
                break;
            }
            let step = i32::try_from(rows.min(offset)).unwrap_or(i32::MAX);
            self.projection.scroll_by_rows(-step);
            self.session.refresh_projection(&mut self.projection);
        }
        eprintln!(
            "REVIEW_AUDIT pages={pages} rendered_history={total_rendered_history} source_rows={total_source_rows}"
        );
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
                "GEOM frame={} occ_id={:?} band={}..={} occ_rows={} clip_top={} clip_bot={} frozen_prefix={} scale={} top_sub={} content_off={} clip_h={} art_h={} pad={} src=\"{}\"",
                self.frame_sequence,
                occurrence,
                band_start,
                band_end,
                block.occluded_source_rows,
                block.clipped_top_rows,
                block.clipped_bottom_rows,
                block.frozen_prefix_rows,
                block.artifact.render_scale_milli,
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
    // Optional source-integrity annotations (batch ⑥ layer 1): a sidecar naming known upstream
    // byte-damaged source rows so the containment gate tolerates exactly those orphans. Absent =
    // no annotations (every orphan reds). One record per line: `history <id> <note>` or
    // `grid <row> <note>`; `#` comments and blank lines ignored.
    if let Some(path) = env::var_os("BT_PROBE_ANNOTATIONS") {
        oracle.annotations = load_source_integrity_annotations(Path::new(&path))?;
    }
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

    if env::var_os("BT_PROBE_REVIEW").is_some() {
        oracle.review_render_audit()?;
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
            // Each frozen row's decoration state and failure reason, so a replay directly names why a
            // block shows source: `pending` (the scheduling-loss liveness hole), `failed` with the
            // bt-math reason, `suppressed`, or a `none` candidate that never re-scheduled. Mirrors the
            // runtime `BT_DECOR_TRACE` labels.
            let (state, reason) =
                oracle
                    .session
                    .decoration(line.id)
                    .map_or(("absent", "-"), |record| {
                        (
                            bt_term::decoration_state_label(record.decoration),
                            record.failure_reason.as_deref().unwrap_or("-"),
                        )
                    });
            eprintln!(
                "FROZEN[{}] state={state} reason={reason} |{}",
                line.id.0, line.text
            );
            if env::var_os("BT_PROBE_STYLES").is_some() {
                for span in &line.styles {
                    eprintln!("  STYLE {span:?}");
                }
            }
        }
    }

    // Document-level detection red gate (live-norender audit tool-gap): blocks provable by a clean
    // grid-only re-scan but stranded at source by a poisoned frozen prefix. The flash oracle is
    // blind to these (a block never placed leaves no placement history). A healthy run is 0/0.
    let final_isolation_gap = oracle.session.live_detection_isolation_gap();
    eprintln!(
        "ISOLATION_GAP final={final_isolation_gap} max={}",
        oracle.max_isolation_gap
    );

    // Batch ⑥ split red gate. The ownership ledger accounts every structural delimiter as owned by a
    // detected block, a legitimate rejection, or an orphan (hold-independent). The two layers:
    //   * source-integrity — known upstream byte damage, annotated per recording, reported not red;
    //   * detector-containment — any UNANNOTATED orphan is a containment failure and reds the exit.
    let final_ledger = oracle.session.live_detection_ownership_ledger();
    let verdict = final_ledger.containment(&oracle.annotations);
    eprintln!(
        "OWNERSHIP_LEDGER detected={} rejections={} orphans={} annotated={} clipped_open={} max_orphans={} ever_clipped_open={}",
        verdict.detected,
        verdict.legitimate_rejections,
        verdict.orphans,
        verdict.annotated_damage,
        verdict.clipped_open,
        oracle.max_orphans,
        oracle.ever_clipped_open,
    );
    for entry in final_ledger.orphan_entries() {
        let annotated = entry.source_line.is_some_and(|line| {
            oracle
                .annotations
                .iter()
                .any(|annotation| annotation.source_line == line)
        });
        eprintln!(
            "  ORPHAN kind={:?} source_line={:?} logical_index={} dependency=[{},{}] annotated={annotated}",
            entry.fate,
            entry.source_line,
            entry.logical_index,
            entry.dependency_start,
            entry.dependency_end,
        );
    }
    for annotation in &oracle.annotations {
        eprintln!(
            "  SOURCE_INTEGRITY source_line={:?} note={:?}",
            annotation.source_line, annotation.note
        );
    }
    if env::var_os("BT_PROBE_OWNERSHIP_DUMP").is_some() {
        for entry in &final_ledger.entries {
            eprintln!(
                "  LEDGER li={} kind={:?} fate={:?} source_line={:?}",
                entry.logical_index, entry.kind, entry.fate, entry.source_line
            );
        }
    }

    // Batch ③ stale-hold honesty (review §4). Report every formula still displayed at the final,
    // quiescent state whose exact source the current scan no longer Owns — a hold masking dead
    // detection. Annotated known-legitimate long-lived forms are counted separately, never as a
    // blanket waiver. `final` is the hold-masking strand count; `max` is the peak transient across the
    // run. Display behaviour is unchanged: this only observes.
    let final_held_unbacked = oracle.session.held_unbacked_records();
    let final_unannotated = final_held_unbacked
        .iter()
        .filter(|record| !held_unbacked_is_annotated(record, &oracle.annotations))
        .count();
    let final_annotated = final_held_unbacked.len() - final_unannotated;
    eprintln!(
        "HELD_UNBACKED final={final_unannotated} annotated={final_annotated} max={}",
        oracle.max_held_unbacked
    );
    for record in &final_held_unbacked {
        let annotated = held_unbacked_is_annotated(record, &oracle.annotations);
        eprintln!(
            "  HELD_UNBACKED source_line={:?} screen={:?} band=[{},{}] stale={} annotated={annotated} source={:?}",
            record.source_line,
            record.screen,
            record.band_start_row,
            record.band_end_row,
            record.stale,
            record.original_source,
        );
    }

    // Emit the geometry and source-occlusion summaries before the historical flash gate. A replay
    // may contain an unrelated known R->S flip; that must not hide these independent diagnostics.
    eprintln!(
        "OCCLUSION_AUDIT max_residue_cells={} worst={}",
        oracle.max_occlusion_residue_cells,
        oracle.occlusion_residue_worst.as_deref().unwrap_or("none")
    );
    eprintln!(
        "CLIP_AUDIT max_misalign={} worst={}",
        oracle.max_clip_misalign,
        oracle.clip_misalign_worst.as_deref().unwrap_or("none")
    );
    eprintln!(
        "LAYOUT_AUDIT max_borrowed_band_rows={} max_overlapping_live_bands={} final_visible_source_rows={} worst={}",
        oracle.max_borrowed_band_rows,
        oracle.max_overlapping_live_bands,
        oracle.final_visible_source_rows,
        oracle.layout_worst.as_deref().unwrap_or("none"),
    );
    if env::var_os("BT_PROBE_OCCLUSION_AUDIT").is_some() && oracle.max_occlusion_residue_cells > 0 {
        return Err(io::Error::other(format!(
            "terminal presentation residue under rendered math: {} cell(s) (worst: {})",
            oracle.max_occlusion_residue_cells,
            oracle.occlusion_residue_worst.as_deref().unwrap_or("none")
        ))
        .into());
    }
    if env::var_os("BT_PROBE_CLIP_AUDIT").is_some() && oracle.max_clip_misalign > 0 {
        return Err(io::Error::other(format!(
            "stale-window clip misalignment: {} primary block(s) clipped short of their artifact with no genuine-clip context (worst: {})",
            oracle.max_clip_misalign,
            oracle.clip_misalign_worst.as_deref().unwrap_or("none")
        ))
        .into());
    }
    if env::var_os("BT_PROBE_LAYOUT_AUDIT").is_some()
        && (oracle.max_borrowed_band_rows > 0
            || oracle.max_overlapping_live_bands > 0
            || oracle.final_visible_source_rows > 0)
    {
        return Err(io::Error::other(format!(
            "live formula layout ownership failed: borrowed_rows={} overlaps={} final_source_rows={} (worst: {})",
            oracle.max_borrowed_band_rows,
            oracle.max_overlapping_live_bands,
            oracle.final_visible_source_rows,
            oracle.layout_worst.as_deref().unwrap_or("none"),
        ))
        .into());
    }

    if oracle.flash_oracle.flash_detected() {
        return Err(io::Error::other(format!(
            "formula repaint flash detected for {:?}",
            oracle.flash_oracle.flashed_sources()
        ))
        .into());
    }

    // Enforce the detector-containment gate as a real exit criterion (opt-in so it never surprises an
    // unrelated regression run, and so the batch-2 baseline recordings can be driven explicitly). The
    // hard gate is the FINAL-state verdict: a hold-independent unannotated orphan that persists to
    // quiescence is a genuine strand. A transient mid-stream orphan (a block whose opener is on the
    // grid before its body and closer have streamed in) is legitimate and resolves, so `max_orphans`
    // is a reported diagnostic, not a gate — reddening it would false-fail a healthy streaming run.
    if env::var_os("BT_PROBE_OWNERSHIP").is_some() && verdict.red {
        return Err(io::Error::other(format!(
            "detector-containment failure: {} unannotated orphan(s) stranded at final state (clipped_open={})",
            verdict.orphans, verdict.clipped_open
        ))
        .into());
    }

    // Batch ③ stale-hold gate (opt-in). `HeldUnbacked` is a signal, not a hard red in flight: it
    // legitimately appears mid-reprint / mid-resize / mid-stream, exactly the transients the holds
    // exist to bridge. Only a `HeldUnbacked` that PERSISTS to the quiescent final state — a hold still
    // showing a formula the settled detector no longer accounts — is a genuine masked-dead-detection
    // strand and reds the exit. Known-legitimate long-lived forms are exempted precisely by source
    // annotation (`final_annotated`), never by a blanket waiver, so a NEW unannotated strand still
    // reds. Opt-in so it never surprises an unrelated regression run.
    if env::var_os("BT_PROBE_HELD_UNBACKED").is_some() && final_unannotated > 0 {
        return Err(io::Error::other(format!(
            "stale-hold failure: {final_unannotated} unannotated HeldUnbacked formula(s) still displayed at final state with no backing detection"
        ))
        .into());
    }

    Ok(())
}

/// A `HeldUnbacked` record is a known-legitimate long-lived form when a source-integrity annotation
/// names its exact opener source row. Batch ③ reuses the batch-⑥ annotation sidecar so a documented
/// strand is exempted precisely, never by a blanket waiver.
fn held_unbacked_is_annotated(
    record: &bt_term::HeldUnbackedRecord,
    annotations: &[bt_detect::SourceIntegrityAnnotation],
) -> bool {
    annotations
        .iter()
        .any(|annotation| annotation.source_line == record.source_line)
}

/// Parse a source-integrity annotation sidecar. Each non-comment line is `history <id> <note>` or
/// `grid <row> <note>`, naming a reconstructed source row known to carry (or to be missing) an
/// upstream-damaged delimiter. These are documentation of layer-1 damage; the containment gate
/// tolerates an orphan only when its exact source row is named here.
fn load_source_integrity_annotations(
    path: &Path,
) -> Result<Vec<bt_detect::SourceIntegrityAnnotation>, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let mut annotations = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, char::is_whitespace);
        let kind = parts.next().unwrap_or_default();
        let number = parts.next().unwrap_or_default();
        let note = parts.next().unwrap_or_default().to_owned();
        let source_line = match kind {
            "history" => {
                bt_detect::MathSourceLine::Transcript(bt_transcript::TranscriptId(number.parse()?))
            }
            "grid" => bt_detect::MathSourceLine::LiveGrid(number.parse()?),
            other => {
                return Err(io::Error::other(format!(
                    "unknown annotation kind {other:?} (expected `history` or `grid`)"
                ))
                .into());
            }
        };
        annotations.push(bt_detect::SourceIntegrityAnnotation { source_line, note });
    }
    Ok(annotations)
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
