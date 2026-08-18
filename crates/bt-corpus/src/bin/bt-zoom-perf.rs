use std::{
    env, fs, io,
    num::NonZeroU32,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use bt_render::{HeadlessRenderProbe, RenderProbeSample};
use bt_term::{
    DualPlaneSession, LayoutKey, MathEngine, SessionMathTask, render_detection_task,
    render_live_detection_task,
};
use bt_viewport::ViewportProjection;

const FOREGROUND_RGB: [u8; 3] = [0xd8, 0xdc, 0xe8];
const HEADLESS_WIDTH: u32 = 1_600;
const HEADLESS_HEIGHT: u32 = 1_000;
const STALE_REPEAT_FRAMES: usize = 5;

struct ReplayChunk<'a> {
    sequence: u64,
    elapsed: Duration,
    bytes: &'a [u8],
    resize_before: Option<(NonZeroU32, NonZeroU32)>,
}

#[derive(Clone, Copy, Debug, Default)]
struct WorkerStats {
    elapsed: Duration,
    frozen: u64,
    live: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct ZoomSample {
    scale_factor: Duration,
    metric_push: Duration,
    resize: Duration,
    layout_key: Duration,
    projection: Duration,
    frame_build: Duration,
    stale_render: Duration,
    stale_repeat_projection: Duration,
    stale_repeat_frame_build: Duration,
    stale_repeat_render: Duration,
    worker: WorkerStats,
    fresh_projection: Duration,
    fresh_frame_build: Duration,
    fresh_render: Duration,
    stale_render_stats: RenderStats,
    stale_repeat_render_stats: RenderStats,
    fresh_render_stats: RenderStats,
    frozen_rows: usize,
    math_blocks: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct RenderStats {
    row_compose: Duration,
    atlas_prepare_upload: Duration,
    math_prepare_upload: Duration,
    encode_submit: Duration,
    rows_reshaped: u64,
    math_texture_uploads: u64,
    math_texture_upload_bytes: usize,
}

impl From<RenderProbeSample> for RenderStats {
    fn from(sample: RenderProbeSample) -> Self {
        Self {
            row_compose: sample.row_compose,
            atlas_prepare_upload: sample.atlas_prepare_upload,
            math_prepare_upload: sample.math_prepare_upload,
            encode_submit: sample.encode_submit,
            rows_reshaped: sample.rows_reshaped,
            math_texture_uploads: sample.math_texture_uploads,
            math_texture_upload_bytes: sample.math_texture_upload_bytes,
        }
    }
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let input_path = args
        .next()
        .map(PathBuf::from)
        .context("usage: bt-zoom-perf INPUT.vt COLUMNS ROWS [SAMPLES]")?;
    let columns = parse_dimension(args.next().as_deref(), "COLUMNS")?;
    let rows = parse_dimension(args.next().as_deref(), "ROWS")?;
    let samples = args
        .next()
        .map(|value| value.parse::<usize>().context("SAMPLES must be an integer"))
        .transpose()?
        .unwrap_or(8);
    if samples < 2 {
        bail!("SAMPLES must be at least 2 so both zoom directions are measured");
    }

    let input = fs::read(&input_path)
        .with_context(|| format!("read replay input {}", input_path.display()))?;
    let chunks_path = append_suffix(&input_path, ".chunks");
    let chunks = if chunks_path.is_file() {
        parse_chunks(&input, &fs::read_to_string(&chunks_path)?)?
    } else {
        vec![ReplayChunk {
            sequence: 0,
            elapsed: Duration::ZERO,
            bytes: &input,
            resize_before: None,
        }]
    };
    let stop_sequence = env::var("BT_ZOOM_PERF_STOP_SEQUENCE")
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .context("BT_ZOOM_PERF_STOP_SEQUENCE must be an integer")
        })
        .transpose()?;

    let started = Instant::now();
    let mut session = DualPlaneSession::new(columns, rows);
    let mut projection = session.new_projection(session.layout_key());
    let engine = MathEngine::new();
    let mut last_elapsed = Duration::ZERO;
    for chunk in chunks {
        if stop_sequence.is_some_and(|stop| chunk.sequence > stop) {
            break;
        }
        last_elapsed = chunk.elapsed;
        let observed_at = started + chunk.elapsed;
        let _ = session.finish_resize_if_quiescent(observed_at);
        session.advance_live_stability(observed_at);
        complete_pending_math(&mut session, &engine);
        if let Some((next_columns, next_rows)) = chunk.resize_before {
            session.resize_at(next_columns, next_rows, observed_at)?;
            session.mark_pty_resize_requested_at(next_columns, next_rows, observed_at);
        }
        session.feed_at(chunk.bytes, observed_at)?;
        complete_pending_math(&mut session, &engine);
    }
    let settled_at = started + last_elapsed + Duration::from_secs(1);
    let _ = session.finish_resize_if_quiescent(settled_at);
    session.advance_live_stability(settled_at);
    complete_pending_math(&mut session, &engine);
    session.refresh_projection(&mut projection);
    let steady_frame = session.viewport_frame(&mut projection)?;
    print_geometry("steady", &steady_frame);

    let mut probe = pollster::block_on(HeadlessRenderProbe::new(
        HEADLESS_WIDTH,
        HEADLESS_HEIGHT,
        1.0,
    ))?;
    let render_frames = env::var_os("BT_ZOOM_PERF_CPU_ONLY").is_none();
    if render_frames {
        let _ = probe.prepare_frame(&steady_frame)?;
    }
    let base_dimensions = session.terminal().dimensions();
    let zoomed_dimensions = (
        env_dimension("BT_ZOOM_PERF_TARGET_COLUMNS")?
            .unwrap_or_else(|| scale_dimension(base_dimensions.0, 5, 4)),
        env_dimension("BT_ZOOM_PERF_TARGET_ROWS")?
            .unwrap_or_else(|| scale_dimension(base_dimensions.1, 5, 4)),
    );
    let zoom_out_scale = env::var("BT_ZOOM_PERF_TARGET_SCALE")
        .ok()
        .map(|value| {
            value
                .parse::<f64>()
                .context("BT_ZOOM_PERF_TARGET_SCALE must be a number")
        })
        .transpose()?
        .unwrap_or(0.8);
    if let Some(rows) = env::var("BT_ZOOM_PERF_SCROLL_ROWS")
        .ok()
        .map(|value| {
            value
                .parse::<i32>()
                .context("BT_ZOOM_PERF_SCROLL_ROWS must be an integer")
        })
        .transpose()?
    {
        projection.scroll_by_rows(rows);
        session.refresh_projection(&mut projection);
        let reviewed_frame = session.viewport_frame(&mut projection)?;
        print_geometry("reviewed-steady", &reviewed_frame);
    }
    eprintln!(
        "BT_ZOOM_PERF_SETUP case={} bytes={} frozen_rows={} base={}x{} zoomed={}x{} steady_math_blocks={} adapter={:?}",
        input_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown"),
        input.len(),
        session.transcript().frozen().len(),
        base_dimensions.0,
        base_dimensions.1,
        zoomed_dimensions.0,
        zoomed_dimensions.1,
        steady_frame.math_blocks.len(),
        probe.adapter_name(),
    );

    let mut zoom_out = Vec::new();
    let mut zoom_in = Vec::new();
    for index in 0..samples {
        let out = index % 2 == 0;
        let (scale, dimensions) = if out {
            (zoom_out_scale, zoomed_dimensions)
        } else {
            (1.0, base_dimensions)
        };
        let sample = run_zoom_sample(
            &mut session,
            &mut projection,
            &mut probe,
            &engine,
            scale,
            dimensions,
            settled_at + Duration::from_secs(index as u64 + 1),
            render_frames,
        )?;
        print_sample(
            input_path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown"),
            if out { "out" } else { "in" },
            index / 2,
            sample,
        );
        if out {
            zoom_out.push(sample);
        } else {
            zoom_in.push(sample);
        }
    }
    print_summary(&input_path, "out", &zoom_out);
    print_summary(&input_path, "in", &zoom_in);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_zoom_sample(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    probe: &mut HeadlessRenderProbe,
    engine: &MathEngine,
    scale: f64,
    dimensions: (NonZeroU32, NonZeroU32),
    observed_at: Instant,
    render_frames: bool,
) -> Result<ZoomSample> {
    let started = Instant::now();
    let metrics = probe.update_scale_factor(scale)?;
    let scale_factor = started.elapsed();

    let started = Instant::now();
    session.set_cell_height_subpixels(metrics.cell_height_subpixels());
    session.set_cell_width_subpixels(metrics.cell_width_subpixels());
    session.set_ascii_baseline_subpixels(metrics.ascii_baseline_subpixels());
    let metric_push = started.elapsed();

    let started = Instant::now();
    session.resize_at(dimensions.0, dimensions.1, observed_at)?;
    let resize = started.elapsed();

    let started = Instant::now();
    session.set_layout_key(LayoutKey {
        width_cells: dimensions.0,
        dpi_milli: metrics.dpi_milli(),
        font_rev: 1,
        theme_rev: session.layout_key().theme_rev,
        lang_rev: session.layout_key().lang_rev,
        profile_rev: session.layout_key().profile_rev,
    });
    let layout_key = started.elapsed();

    let started = Instant::now();
    session.refresh_projection(projection);
    let projection_elapsed = started.elapsed();
    let started = Instant::now();
    let stale_frame = session.viewport_frame(projection)?;
    print_geometry("stale", &stale_frame);
    let frame_build = started.elapsed();
    let started = Instant::now();
    let stale_render_sample = if render_frames {
        probe.prepare_frame(&stale_frame)?
    } else {
        RenderProbeSample::default()
    };
    let stale_render = started.elapsed();

    let mut repeat_projection = Duration::ZERO;
    let mut repeat_frame_build = Duration::ZERO;
    let mut repeat_render = Duration::ZERO;
    let mut repeat_stats = RenderStats::default();
    for _ in 0..STALE_REPEAT_FRAMES {
        let started = Instant::now();
        session.refresh_projection(projection);
        repeat_projection += started.elapsed();
        let started = Instant::now();
        let frame = session.viewport_frame(projection)?;
        repeat_frame_build += started.elapsed();
        let started = Instant::now();
        let render = if render_frames {
            probe.prepare_frame(&frame)?
        } else {
            RenderProbeSample::default()
        };
        repeat_render += started.elapsed();
        accumulate_render_stats(&mut repeat_stats, render.into());
    }

    let worker = complete_pending_math(session, engine);
    let started = Instant::now();
    session.refresh_projection(projection);
    let fresh_projection = started.elapsed();
    let started = Instant::now();
    let fresh_frame = session.viewport_frame(projection)?;
    print_geometry("fresh", &fresh_frame);
    let fresh_frame_build = started.elapsed();
    let started = Instant::now();
    let fresh_render_sample = if render_frames {
        probe.prepare_frame(&fresh_frame)?
    } else {
        RenderProbeSample::default()
    };
    let fresh_render = started.elapsed();

    let sample = ZoomSample {
        scale_factor,
        metric_push,
        resize,
        layout_key,
        projection: projection_elapsed,
        frame_build,
        stale_render,
        stale_repeat_projection: repeat_projection / STALE_REPEAT_FRAMES as u32,
        stale_repeat_frame_build: repeat_frame_build / STALE_REPEAT_FRAMES as u32,
        stale_repeat_render: repeat_render / STALE_REPEAT_FRAMES as u32,
        worker,
        fresh_projection,
        fresh_frame_build,
        fresh_render,
        stale_render_stats: stale_render_sample.into(),
        stale_repeat_render_stats: divide_render_stats(repeat_stats, STALE_REPEAT_FRAMES as u64),
        fresh_render_stats: fresh_render_sample.into(),
        frozen_rows: session.transcript().frozen().len(),
        math_blocks: stale_frame.math_blocks.len(),
    };
    session.mark_pty_resize_requested_at(dimensions.0, dimensions.1, observed_at);
    let quiesced_at = observed_at + Duration::from_secs(1);
    let _ = session.finish_resize_if_quiescent(quiesced_at)?;
    session.advance_live_stability(quiesced_at);
    complete_pending_math(session, engine);
    Ok(sample)
}

fn complete_pending_math(session: &mut DualPlaneSession, engine: &MathEngine) -> WorkerStats {
    let started = Instant::now();
    let mut stats = WorkerStats::default();
    while let Some(task) = session.take_math_worker_task() {
        match task {
            SessionMathTask::Frozen(mut task) => {
                stats.frozen = stats.frozen.saturating_add(1);
                let result = render_detection_task(engine, &mut task, FOREGROUND_RGB);
                session.complete_worker_result(task, result);
            }
            SessionMathTask::Live(mut task) => {
                stats.live = stats.live.saturating_add(1);
                let result = render_live_detection_task(engine, &mut task, FOREGROUND_RGB);
                session.complete_live_worker_result(task, result);
            }
        }
    }
    stats.elapsed = started.elapsed();
    stats
}

fn accumulate_render_stats(total: &mut RenderStats, sample: RenderStats) {
    total.row_compose += sample.row_compose;
    total.atlas_prepare_upload += sample.atlas_prepare_upload;
    total.math_prepare_upload += sample.math_prepare_upload;
    total.encode_submit += sample.encode_submit;
    total.rows_reshaped = total.rows_reshaped.saturating_add(sample.rows_reshaped);
    total.math_texture_uploads = total
        .math_texture_uploads
        .saturating_add(sample.math_texture_uploads);
    total.math_texture_upload_bytes = total
        .math_texture_upload_bytes
        .saturating_add(sample.math_texture_upload_bytes);
}

fn divide_render_stats(stats: RenderStats, divisor: u64) -> RenderStats {
    RenderStats {
        row_compose: stats.row_compose / divisor as u32,
        atlas_prepare_upload: stats.atlas_prepare_upload / divisor as u32,
        math_prepare_upload: stats.math_prepare_upload / divisor as u32,
        encode_submit: stats.encode_submit / divisor as u32,
        rows_reshaped: stats.rows_reshaped / divisor,
        math_texture_uploads: stats.math_texture_uploads / divisor,
        math_texture_upload_bytes: stats.math_texture_upload_bytes / divisor as usize,
    }
}

fn print_sample(case: &str, direction: &str, iteration: usize, sample: ZoomSample) {
    eprintln!(
        "BT_ZOOM_PERF_SAMPLE case={case} direction={direction} iteration={iteration} frozen_rows={} math_blocks={} scale_us={} metric_push_us={} resize_us={} layout_key_us={} projection_us={} frame_build_us={} stale_render_us={} stale_row_compose_us={} stale_atlas_us={} stale_math_upload_us={} stale_math_uploads={} stale_math_upload_bytes={} stale_repeat_projection_us={} stale_repeat_frame_build_us={} stale_repeat_render_us={} stale_repeat_row_compose_us={} stale_repeat_atlas_us={} stale_repeat_math_upload_us={} stale_repeat_encode_submit_us={} stale_repeat_rows_reshaped={} stale_repeat_math_uploads={} worker_us={} worker_frozen={} worker_live={} fresh_projection_us={} fresh_frame_build_us={} fresh_render_us={} fresh_math_uploads={}",
        sample.frozen_rows,
        sample.math_blocks,
        sample.scale_factor.as_micros(),
        sample.metric_push.as_micros(),
        sample.resize.as_micros(),
        sample.layout_key.as_micros(),
        sample.projection.as_micros(),
        sample.frame_build.as_micros(),
        sample.stale_render.as_micros(),
        sample.stale_render_stats.row_compose.as_micros(),
        sample.stale_render_stats.atlas_prepare_upload.as_micros(),
        sample.stale_render_stats.math_prepare_upload.as_micros(),
        sample.stale_render_stats.math_texture_uploads,
        sample.stale_render_stats.math_texture_upload_bytes,
        sample.stale_repeat_projection.as_micros(),
        sample.stale_repeat_frame_build.as_micros(),
        sample.stale_repeat_render.as_micros(),
        sample.stale_repeat_render_stats.row_compose.as_micros(),
        sample
            .stale_repeat_render_stats
            .atlas_prepare_upload
            .as_micros(),
        sample
            .stale_repeat_render_stats
            .math_prepare_upload
            .as_micros(),
        sample.stale_repeat_render_stats.encode_submit.as_micros(),
        sample.stale_repeat_render_stats.rows_reshaped,
        sample.stale_repeat_render_stats.math_texture_uploads,
        sample.worker.elapsed.as_micros(),
        sample.worker.frozen,
        sample.worker.live,
        sample.fresh_projection.as_micros(),
        sample.fresh_frame_build.as_micros(),
        sample.fresh_render.as_micros(),
        sample.fresh_render_stats.math_texture_uploads,
    );
}

fn print_summary(input: &Path, direction: &str, samples: &[ZoomSample]) {
    if samples.is_empty() {
        return;
    }
    let case = input
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    eprintln!(
        "BT_ZOOM_PERF_SUMMARY case={case} direction={direction} samples={} instant_total_p50_us={} scale_p50_us={} resize_p50_us={} layout_key_p50_us={} projection_p50_us={} frame_build_p50_us={} stale_render_p50_us={} stale_math_upload_p50_us={} stale_math_uploads_p50={} stale_math_upload_bytes_p50={} stale_repeat_total_p50_us={} stale_repeat_projection_p50_us={} stale_repeat_frame_build_p50_us={} stale_repeat_render_p50_us={} stale_repeat_row_compose_p50_us={} stale_repeat_atlas_p50_us={} stale_repeat_math_upload_p50_us={} stale_repeat_encode_submit_p50_us={} stale_repeat_math_uploads_p50={} worker_p50_us={} fresh_projection_p50_us={} fresh_frame_build_p50_us={} fresh_render_p50_us={}",
        samples.len(),
        median(samples, |sample| {
            sample.scale_factor
                + sample.metric_push
                + sample.resize
                + sample.layout_key
                + sample.projection
                + sample.frame_build
                + sample.stale_render
        }),
        median(samples, |sample| sample.scale_factor),
        median(samples, |sample| sample.resize),
        median(samples, |sample| sample.layout_key),
        median(samples, |sample| sample.projection),
        median(samples, |sample| sample.frame_build),
        median(samples, |sample| sample.stale_render),
        median(samples, |sample| {
            sample.stale_render_stats.math_prepare_upload
        }),
        median_u64(samples, |sample| {
            sample.stale_render_stats.math_texture_uploads
        }),
        median_usize(samples, |sample| {
            sample.stale_render_stats.math_texture_upload_bytes
        }),
        median(samples, |sample| {
            sample.stale_repeat_projection
                + sample.stale_repeat_frame_build
                + sample.stale_repeat_render
        }),
        median(samples, |sample| sample.stale_repeat_projection),
        median(samples, |sample| sample.stale_repeat_frame_build),
        median(samples, |sample| sample.stale_repeat_render),
        median(samples, |sample| {
            sample.stale_repeat_render_stats.row_compose
        }),
        median(samples, |sample| {
            sample.stale_repeat_render_stats.atlas_prepare_upload
        }),
        median(samples, |sample| {
            sample.stale_repeat_render_stats.math_prepare_upload
        }),
        median(samples, |sample| {
            sample.stale_repeat_render_stats.encode_submit
        }),
        median_u64(samples, |sample| {
            sample.stale_repeat_render_stats.math_texture_uploads
        }),
        median(samples, |sample| sample.worker.elapsed),
        median(samples, |sample| sample.fresh_projection),
        median(samples, |sample| sample.fresh_frame_build),
        median(samples, |sample| sample.fresh_render),
    );
}

fn median(samples: &[ZoomSample], value: impl Fn(&ZoomSample) -> Duration) -> u128 {
    let mut values = samples
        .iter()
        .map(|sample| value(sample).as_micros())
        .collect::<Vec<_>>();
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_u64(samples: &[ZoomSample], value: impl Fn(&ZoomSample) -> u64) -> u64 {
    let mut values = samples.iter().map(value).collect::<Vec<_>>();
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_usize(samples: &[ZoomSample], value: impl Fn(&ZoomSample) -> usize) -> usize {
    let mut values = samples.iter().map(value).collect::<Vec<_>>();
    values.sort_unstable();
    values[values.len() / 2]
}

fn parse_dimension(value: Option<&str>, name: &str) -> Result<NonZeroU32> {
    let value = value
        .with_context(|| format!("{name} is required"))?
        .parse::<u32>()
        .with_context(|| format!("{name} must be an integer"))?;
    NonZeroU32::new(value).with_context(|| format!("{name} must be non-zero"))
}

fn env_dimension(name: &str) -> Result<Option<NonZeroU32>> {
    env::var(name)
        .ok()
        .map(|value| {
            let value = value
                .parse::<u32>()
                .with_context(|| format!("{name} must be an integer"))?;
            NonZeroU32::new(value).with_context(|| format!("{name} must be non-zero"))
        })
        .transpose()
}

fn print_geometry(stage: &str, frame: &bt_viewport::ViewportFrame) {
    if env::var_os("BT_ZOOM_PERF_GEOMETRY").is_none() {
        return;
    }
    eprintln!(
        "BT_ZOOM_GEOMETRY stage={stage} rows={} scroll_offset={} blocks={}",
        frame.rows,
        frame.scroll_offset_rows,
        frame.math_blocks.len()
    );
    for block in &frame.math_blocks {
        eprintln!(
            "BT_ZOOM_GEOMETRY stage={stage} source={:?} display={:?} top={} content_off={} clip_h={} art_h={} scale={} clip_top={} clip_bottom={} occluded={} band={:?}",
            block.source.chars().take(40).collect::<String>(),
            block.display,
            block.top_subpixels,
            block.content_offset_subpixels,
            block.clip_height_subpixels,
            block.artifact.height_subpixels,
            block.artifact.render_scale_milli,
            block.clipped_top_rows,
            block.clipped_bottom_rows,
            block.occluded_source_rows,
            block.anchor,
        );
    }
}

fn scale_dimension(value: NonZeroU32, numerator: u32, denominator: u32) -> NonZeroU32 {
    let scaled = value
        .get()
        .saturating_mul(numerator)
        .saturating_add(denominator - 1)
        / denominator;
    NonZeroU32::new(scaled.max(1)).expect("scaled dimension is non-zero")
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn parse_chunks<'a>(input: &'a [u8], manifest: &str) -> Result<Vec<ReplayChunk<'a>>> {
    let mut chunks = Vec::new();
    let mut offset = 0usize;
    let mut expected_sequence = 0u64;
    let mut previous_elapsed = Duration::ZERO;
    let mut pending_resize = None;
    for (line_index, line) in manifest.lines().enumerate() {
        let line = line.trim();
        if let Some(resize) = line.strip_prefix("# RESIZE ") {
            let fields = resize.split_ascii_whitespace().collect::<Vec<_>>();
            let columns = fields
                .first()
                .context("resize marker is missing columns")?
                .parse::<u32>()?;
            let rows = fields
                .get(1)
                .context("resize marker is missing rows")?
                .parse::<u32>()?;
            pending_resize = Some((
                NonZeroU32::new(columns).context("resize columns must be non-zero")?,
                NonZeroU32::new(rows).context("resize rows must be non-zero")?,
            ));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!(
                "invalid chunk manifest line {}: expected three fields",
                line_index + 1
            );
        }
        let sequence = fields[0].parse::<u64>()?;
        let elapsed = Duration::from_micros(fields[1].parse::<u64>()?);
        let length = fields[2].parse::<usize>()?;
        if sequence != expected_sequence {
            bail!(
                "invalid chunk manifest line {}: expected sequence {expected_sequence}, got {sequence}",
                line_index + 1
            );
        }
        if elapsed < previous_elapsed {
            bail!(
                "invalid chunk manifest line {}: time moved backwards",
                line_index + 1
            );
        }
        let end = offset
            .checked_add(length)
            .context("chunk length overflowed input offset")?;
        let bytes = input.get(offset..end).with_context(|| {
            format!("chunk manifest line {} exceeds input bytes", line_index + 1)
        })?;
        chunks.push(ReplayChunk {
            sequence,
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
                "chunk manifest accounts for {offset} of {} input bytes",
                input.len()
            ),
        )
        .into());
    }
    Ok(chunks)
}
