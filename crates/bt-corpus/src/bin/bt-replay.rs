use std::{
    cell::RefCell,
    env,
    fs::File,
    num::NonZeroU32,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use bt_corpus::{Chunking, Corpus, EventKind};
use bt_render::{HeadlessRenderProbe, RenderProbeSample};
use bt_term::{DualPlaneSession, TerminalAdapter};
use bt_viewport::{ViewportFrame, ViewportProjection};
use vte::{Params, Parser, Perform};

const SYNTHETIC_COLUMNS: u32 = 100;
const SYNTHETIC_ROWS: u32 = 50;
const SYNTHETIC_LINE_CELLS: usize = 98;
const HEADLESS_WIDTH: u32 = 1_200;
const HEADLESS_HEIGHT: u32 = 1_200;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let first = args.next().context(
        "usage: bt-replay CORPUS.btcr [CHUNK_SIZE] [--render] | \
         bt-replay --synthetic CASE MODE [FRAMES] [CHUNK_SIZE|frame] [frame|chunk]",
    )?;
    if first == "--synthetic" {
        return run_synthetic(args.collect());
    }

    let mut render = false;
    let mut chunking = Chunking::Recorded;
    for arg in args {
        if arg == "--render" {
            render = true;
        } else {
            chunking = Chunking::Fixed(
                arg.parse()
                    .context("chunk size must be an integer or --render")?,
            );
        }
    }
    let corpus = Corpus::read_from(File::open(&first)?)?;
    eprintln!(
        "BT_REPLAY conpty_source={:?}",
        corpus.conpty_source.as_deref().unwrap_or("legacy-unknown")
    );
    print_sequence_stats(&corpus);
    if render {
        replay_with_render(&corpus, chunking)
    } else {
        replay_terminal_only(&corpus, chunking)
    }
}

#[derive(Default)]
struct SequencePerformer {
    csi_s: u64,
    csi_t: u64,
    il: u64,
    dl: u64,
    cup: u64,
    el: u64,
    ed: u64,
    bsu: u64,
    esu: u64,
    sync_active: bool,
}

impl Perform for SequencePerformer {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        match action {
            'S' => self.csi_s = self.csi_s.saturating_add(1),
            'T' => self.csi_t = self.csi_t.saturating_add(1),
            'L' => self.il = self.il.saturating_add(1),
            'M' => self.dl = self.dl.saturating_add(1),
            'H' | 'f' => self.cup = self.cup.saturating_add(1),
            'K' => self.el = self.el.saturating_add(1),
            'J' => self.ed = self.ed.saturating_add(1),
            _ => {}
        }
        let synchronized_update = intermediates == b"?"
            && params
                .iter()
                .next()
                .is_some_and(|parameter| parameter == [2026]);
        if synchronized_update && action == 'h' {
            self.bsu = self.bsu.saturating_add(1);
            self.sync_active = true;
        } else if synchronized_update && action == 'l' {
            self.esu = self.esu.saturating_add(1);
            self.sync_active = false;
        }
    }
}

fn print_sequence_stats(corpus: &Corpus) {
    let mut parser = Parser::new();
    let mut performer = SequencePerformer::default();
    let mut output_events = 0_u64;
    let mut output_bytes = Vec::new();
    let mut sync_event_boundaries = 0_u64;
    let mut previous_output_at = None;
    let mut intervals = Vec::new();
    for event in &corpus.events {
        let EventKind::Output(bytes) = &event.kind else {
            continue;
        };
        output_events = output_events.saturating_add(1);
        if let Some(previous) = previous_output_at {
            intervals.push(event.at_micros.saturating_sub(previous));
        }
        previous_output_at = Some(event.at_micros);
        parser.advance(&mut performer, bytes);
        if performer.sync_active {
            sync_event_boundaries = sync_event_boundaries.saturating_add(1);
        }
        output_bytes.extend_from_slice(bytes);
    }
    intervals.sort_unstable();
    let cjk_scalars = String::from_utf8_lossy(&output_bytes)
        .chars()
        .filter(|character| {
            matches!(
                *character,
                '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
            )
        })
        .count();
    eprintln!(
        "BT_REPLAY_SEQUENCES output_events={} output_bytes={} cjk_scalars={} csi_s={} csi_t={} il={} dl={} cup={} el={} ed={} bsu={} esu={} sync_event_boundaries={} output_interval_p50_us={} output_interval_p95_us={}",
        output_events,
        output_bytes.len(),
        cjk_scalars,
        performer.csi_s,
        performer.csi_t,
        performer.il,
        performer.dl,
        performer.cup,
        performer.el,
        performer.ed,
        performer.bsu,
        performer.esu,
        sync_event_boundaries,
        percentile_u64(&intervals, 50),
        percentile_u64(&intervals, 95),
    );
}

fn percentile_u64(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = (sorted.len() - 1) * percentile / 100;
    sorted[index]
}

fn replay_terminal_only(corpus: &Corpus, chunking: Chunking) -> Result<()> {
    let columns = NonZeroU32::new(u32::from(corpus.initial_cols))
        .context("corpus initial columns must be non-zero")?;
    let rows = NonZeroU32::new(u32::from(corpus.initial_rows))
        .context("corpus initial rows must be non-zero")?;
    let terminal = RefCell::new(TerminalAdapter::new(columns, rows));
    corpus.replay(
        chunking,
        |bytes| {
            terminal.borrow_mut().feed(bytes);
        },
        |cols, rows| {
            terminal
                .borrow_mut()
                .resize(NonZeroU32::from(cols), NonZeroU32::from(rows));
        },
    )?;
    for line in terminal.borrow().visible_text() {
        println!("{line}");
    }
    Ok(())
}

fn replay_with_render(corpus: &Corpus, chunking: Chunking) -> Result<()> {
    let columns = NonZeroU32::new(u32::from(corpus.initial_cols))
        .context("corpus initial columns must be non-zero")?;
    let rows = NonZeroU32::new(u32::from(corpus.initial_rows))
        .context("corpus initial rows must be non-zero")?;
    let session = DualPlaneSession::new(columns, rows);
    let projection = session.new_projection(session.layout_key());
    let probe = pollster::block_on(HeadlessRenderProbe::new(
        HEADLESS_WIDTH,
        HEADLESS_HEIGHT,
        1.0,
    ))?;
    eprintln!(
        "BT_REPLAY_GPU adapter={:?} max_texture_dimension_2d={}",
        probe.adapter_name(),
        probe.max_texture_dimension_2d()
    );
    let state = RefCell::new(RenderReplay::new(session, projection, probe));
    corpus.replay(
        chunking,
        |bytes| {
            state.borrow_mut().feed_and_present(bytes).unwrap();
        },
        |cols, rows| {
            state
                .borrow_mut()
                .resize_and_present(NonZeroU32::from(cols), NonZeroU32::from(rows))
                .unwrap();
        },
    )?;
    let state = state.into_inner();
    state.totals.print("corpus", "recorded", "callback");
    for line in state.session.terminal().visible_text() {
        println!("{line}");
    }
    Ok(())
}

struct RenderReplay {
    session: DualPlaneSession,
    projection: ViewportProjection,
    probe: HeadlessRenderProbe,
    totals: PerfTotals,
    last_frame: Option<ViewportFrame>,
}

impl RenderReplay {
    fn new(
        session: DualPlaneSession,
        projection: ViewportProjection,
        probe: HeadlessRenderProbe,
    ) -> Self {
        Self {
            session,
            projection,
            probe,
            totals: PerfTotals::default(),
            last_frame: None,
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        let started = Instant::now();
        self.session.feed(bytes)?;
        self.totals.term += started.elapsed();
        self.totals.feed_calls = self.totals.feed_calls.saturating_add(1);
        self.totals.bytes = self.totals.bytes.saturating_add(bytes.len() as u64);
        Ok(())
    }

    fn feed_and_present(&mut self, bytes: &[u8]) -> Result<()> {
        self.feed(bytes)?;
        self.present()
    }

    fn resize_and_present(&mut self, columns: NonZeroU32, rows: NonZeroU32) -> Result<()> {
        let started = Instant::now();
        self.session.resize(columns, rows)?;
        self.totals.term += started.elapsed();
        self.present()
    }

    fn present(&mut self) -> Result<()> {
        let frame = self.project_frame()?;
        if self
            .last_frame
            .as_ref()
            .is_some_and(|previous| same_visual_frame(previous, &frame))
        {
            self.totals.unchanged_frames = self.totals.unchanged_frames.saturating_add(1);
        }
        let sample = self.probe.prepare_frame(&frame)?;
        self.totals.record(sample);
        self.last_frame = Some(frame);
        Ok(())
    }

    fn present_if_changed(&mut self) -> Result<()> {
        let frame = self.project_frame()?;
        if self
            .last_frame
            .as_ref()
            .is_some_and(|previous| same_visual_frame(previous, &frame))
        {
            self.totals.suppressed_frames = self.totals.suppressed_frames.saturating_add(1);
            return Ok(());
        }
        let sample = self.probe.prepare_frame(&frame)?;
        self.totals.record(sample);
        self.last_frame = Some(frame);
        Ok(())
    }

    fn project_frame(&mut self) -> Result<ViewportFrame> {
        let projection_started = Instant::now();
        self.session.refresh_projection(&mut self.projection);
        let frame = self.session.viewport_frame(&mut self.projection)?;
        self.totals.projection += projection_started.elapsed();
        Ok(frame)
    }

    fn reset_totals(&mut self) {
        self.totals = PerfTotals::default();
    }
}

fn same_visual_frame(previous: &ViewportFrame, next: &ViewportFrame) -> bool {
    previous.columns == next.columns
        && previous.rows == next.rows
        && previous.cells == next.cells
        && previous.cursor == next.cursor
        && previous.selection_spans == next.selection_spans
        && previous.status_text == next.status_text
}

#[derive(Default)]
struct PerfTotals {
    bytes: u64,
    feed_calls: u64,
    term: Duration,
    projection: Duration,
    render: Duration,
    row_compose: Duration,
    shape_cache_miss: Duration,
    atlas_prepare_upload: Duration,
    encode_submit: Duration,
    frames: u64,
    unchanged_frames: u64,
    suppressed_frames: u64,
    rows_reshaped: u64,
    narrow_hits: u64,
    narrow_misses: u64,
    narrow_evictions: u64,
    wide_hits: u64,
    wide_misses: u64,
    wide_evictions: u64,
    render_samples_us: Vec<u128>,
}

impl PerfTotals {
    fn record(&mut self, sample: RenderProbeSample) {
        self.render += sample.total;
        self.row_compose += sample.row_compose;
        self.shape_cache_miss += sample.shape_cache_miss;
        self.atlas_prepare_upload += sample.atlas_prepare_upload;
        self.encode_submit += sample.encode_submit;
        self.frames = self.frames.saturating_add(1);
        self.rows_reshaped = self.rows_reshaped.saturating_add(sample.rows_reshaped);
        self.narrow_hits = self.narrow_hits.saturating_add(sample.narrow_hits);
        self.narrow_misses = self.narrow_misses.saturating_add(sample.narrow_misses);
        self.narrow_evictions = self
            .narrow_evictions
            .saturating_add(sample.narrow_evictions);
        self.wide_hits = self.wide_hits.saturating_add(sample.wide_hits);
        self.wide_misses = self.wide_misses.saturating_add(sample.wide_misses);
        self.wide_evictions = self.wide_evictions.saturating_add(sample.wide_evictions);
        self.render_samples_us.push(sample.total.as_micros());
        if env::var_os("BT_PERF_TRACE").is_some() {
            eprintln!(
                "BT_REPLAY_FRAME frame={} total_us={} row_compose_us={} shape_miss_us={} atlas_prepare_upload_us={} encode_submit_us={} rows_reshaped={} narrow_hits={} narrow_misses={} narrow_evictions={} wide_hits={} wide_misses={} wide_evictions={} narrow_glyphs={} wide_glyphs={}",
                self.frames,
                sample.total.as_micros(),
                sample.row_compose.as_micros(),
                sample.shape_cache_miss.as_micros(),
                sample.atlas_prepare_upload.as_micros(),
                sample.encode_submit.as_micros(),
                sample.rows_reshaped,
                sample.narrow_hits,
                sample.narrow_misses,
                sample.narrow_evictions,
                sample.wide_hits,
                sample.wide_misses,
                sample.wide_evictions,
                sample.narrow_glyphs,
                sample.wide_glyphs,
            );
        }
    }

    fn print(&self, case: &str, mode: &str, present_policy: &str) {
        let mut samples = self.render_samples_us.clone();
        samples.sort_unstable();
        let p50 = percentile(&samples, 50);
        let p95 = percentile(&samples, 95);
        eprintln!(
            "BT_REPLAY_PERF case={case} mode={mode} present_policy={present_policy} bytes={} feed_calls={} frames={} unchanged_frames={} suppressed_frames={} term_total_us={} term_per_feed_us={} projection_total_us={} render_total_us={} render_per_frame_us={} render_p50_us={} render_p95_us={} row_compose_total_us={} shape_miss_total_us={} atlas_prepare_upload_total_us={} encode_submit_total_us={} rows_reshaped={} narrow_hits={} narrow_misses={} narrow_evictions={} wide_hits={} wide_misses={} wide_evictions={} wall_accounted_us={}",
            self.bytes,
            self.feed_calls,
            self.frames,
            self.unchanged_frames,
            self.suppressed_frames,
            self.term.as_micros(),
            average_us(self.term, self.feed_calls),
            self.projection.as_micros(),
            self.render.as_micros(),
            average_us(self.render, self.frames),
            p50,
            p95,
            self.row_compose.as_micros(),
            self.shape_cache_miss.as_micros(),
            self.atlas_prepare_upload.as_micros(),
            self.encode_submit.as_micros(),
            self.rows_reshaped,
            self.narrow_hits,
            self.narrow_misses,
            self.narrow_evictions,
            self.wide_hits,
            self.wide_misses,
            self.wide_evictions,
            (self.term + self.projection + self.render).as_micros(),
        );
    }
}

fn average_us(duration: Duration, count: u64) -> u128 {
    if count == 0 {
        0
    } else {
        duration.as_micros() / u128::from(count)
    }
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = (sorted.len() - 1) * percentile / 100;
    sorted[index]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyntheticCase {
    Ascii,
    Cjk,
    Mixed,
}

impl SyntheticCase {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "ascii" => Ok(Self::Ascii),
            "cjk" => Ok(Self::Cjk),
            "mixed" => Ok(Self::Mixed),
            _ => bail!("synthetic CASE must be ascii, cjk, or mixed"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Ascii => "ascii",
            Self::Cjk => "cjk",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyntheticMode {
    Same,
    Full,
    ScrollUp,
    ScrollDown,
    Replace,
    Alternate,
}

impl SyntheticMode {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "same" => Ok(Self::Same),
            "full" => Ok(Self::Full),
            "scroll-up" => Ok(Self::ScrollUp),
            "scroll-down" => Ok(Self::ScrollDown),
            "replace" => Ok(Self::Replace),
            "alternate" => Ok(Self::Alternate),
            _ => bail!(
                "synthetic MODE must be same, full, scroll-up, scroll-down, replace, or alternate"
            ),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Full => "full",
            Self::ScrollUp => "scroll-up",
            Self::ScrollDown => "scroll-down",
            Self::Replace => "replace",
            Self::Alternate => "alternate",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PresentPolicy {
    Frame,
    Chunk,
    App,
}

impl PresentPolicy {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "frame" => Ok(Self::Frame),
            "chunk" => Ok(Self::Chunk),
            "app" => Ok(Self::App),
            _ => bail!("present policy must be frame, chunk, or app"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Chunk => "chunk",
            Self::App => "app",
        }
    }
}

fn run_synthetic(args: Vec<String>) -> Result<()> {
    let case = SyntheticCase::parse(args.first().map_or("ascii", String::as_str))?;
    let mode = SyntheticMode::parse(args.get(1).map_or("full", String::as_str))?;
    let frames = args
        .get(2)
        .map(|value| value.parse().context("FRAMES must be an integer"))
        .transpose()?
        .unwrap_or(8_usize);
    let chunk_size = match args.get(3).map(String::as_str).unwrap_or("frame") {
        "frame" => None,
        value => Some(
            value
                .parse::<usize>()
                .context("CHUNK_SIZE must be an integer or frame")?,
        ),
    };
    if chunk_size == Some(0) {
        bail!("CHUNK_SIZE must be non-zero");
    }
    let present_policy = PresentPolicy::parse(
        args.get(4)
            .map(String::as_str)
            .unwrap_or(PresentPolicy::Frame.name()),
    )?;
    let unique_cjk = args
        .get(5)
        .map(|value| value.parse().context("UNIQUE_CJK must be an integer"))
        .transpose()?
        .unwrap_or(SYNTHETIC_ROWS as usize * 49);
    if unique_cjk == 0 {
        bail!("UNIQUE_CJK must be non-zero");
    }

    let columns = NonZeroU32::new(SYNTHETIC_COLUMNS).unwrap();
    let rows = NonZeroU32::new(SYNTHETIC_ROWS).unwrap();
    let session = DualPlaneSession::new(columns, rows);
    let projection = session.new_projection(session.layout_key());
    let probe = pollster::block_on(HeadlessRenderProbe::new(
        HEADLESS_WIDTH,
        HEADLESS_HEIGHT,
        1.0,
    ))?;
    eprintln!(
        "BT_REPLAY_GPU adapter={:?} max_texture_dimension_2d={} case={} mode={} frames={} chunk_size={} present_policy={} unique_cjk={}",
        probe.adapter_name(),
        probe.max_texture_dimension_2d(),
        case.name(),
        mode.name(),
        frames,
        chunk_size.map_or_else(|| "frame".to_owned(), |size| size.to_string()),
        present_policy.name(),
        unique_cjk,
    );
    let mut replay = RenderReplay::new(session, projection, probe);
    let mut lines = synthetic_lines(case, 0, unique_cjk);
    let initial = full_repaint(&lines);
    replay.feed(&initial)?;
    replay.present()?;
    let cold = std::mem::take(&mut replay.totals);
    cold.print(case.name(), "cold-fill", "frame");
    replay.reset_totals();

    let mut offset = 0_usize;
    for generation in 1..=frames {
        let payload = match mode {
            SyntheticMode::Same => full_repaint_rotated(&lines, offset),
            SyntheticMode::Full => {
                offset = (offset + 1) % lines.len();
                full_repaint_rotated(&lines, offset)
            }
            SyntheticMode::ScrollUp => {
                let previous_offset = offset;
                offset = (offset + 1) % lines.len();
                scroll_up(&lines[previous_offset])
            }
            SyntheticMode::ScrollDown => {
                offset = (offset + lines.len() - 1) % lines.len();
                scroll_down(&lines[offset])
            }
            SyntheticMode::Replace => {
                lines = synthetic_lines(case, generation, unique_cjk);
                offset = 0;
                full_repaint(&lines)
            }
            SyntheticMode::Alternate => {
                lines = synthetic_lines(case, generation % 2, unique_cjk);
                offset = 0;
                full_repaint(&lines)
            }
        };
        feed_synthetic_payload(&mut replay, &payload, chunk_size, present_policy)?;
    }
    replay
        .totals
        .print(case.name(), mode.name(), present_policy.name());
    Ok(())
}

fn feed_synthetic_payload(
    replay: &mut RenderReplay,
    payload: &[u8],
    chunk_size: Option<usize>,
    present_policy: PresentPolicy,
) -> Result<()> {
    let chunk_size = chunk_size.unwrap_or(payload.len().max(1));
    for chunk in payload.chunks(chunk_size) {
        replay.feed(chunk)?;
        if present_policy == PresentPolicy::Chunk {
            replay.present()?;
        } else if present_policy == PresentPolicy::App
            && replay.session.synchronized_update_deadline().is_none()
        {
            replay.present_if_changed()?;
        }
    }
    if present_policy == PresentPolicy::Frame {
        replay.present()?;
    }
    Ok(())
}

fn synthetic_lines(case: SyntheticCase, generation: usize, unique_cjk: usize) -> Vec<Vec<u8>> {
    (0..SYNTHETIC_ROWS as usize)
        .map(|row| synthetic_line(case, generation, row, unique_cjk))
        .collect()
}

fn synthetic_line(
    case: SyntheticCase,
    generation: usize,
    row: usize,
    unique_cjk: usize,
) -> Vec<u8> {
    match case {
        SyntheticCase::Ascii => ascii_line(generation, row),
        SyntheticCase::Cjk => cjk_line(generation, row, false, unique_cjk),
        SyntheticCase::Mixed => cjk_line(generation, row, true, unique_cjk),
    }
}

fn ascii_line(generation: usize, row: usize) -> Vec<u8> {
    let seed = format!(
        "fn block_{row:02}_{generation:02}() {{ let result = test_case_{row:02}(); assert_eq!(result, 0x{generation:04x}); }} // "
    );
    let plain = pad_ascii(seed, SYNTHETIC_LINE_CELLS);
    let first = 24.min(plain.len());
    let second = 62.min(plain.len());
    format!(
        "\x1b[38;5;81m{}\x1b[38;5;214m{}\x1b[38;5;244m{}\x1b[0m",
        &plain[..first],
        &plain[first..second],
        &plain[second..],
    )
    .into_bytes()
}

fn pad_ascii(mut value: String, width: usize) -> String {
    const FILL: &str = "abcdefghijklmnopqrstuvwxyz_0123456789 ";
    while value.len() < width {
        let remaining = width - value.len();
        value.push_str(&FILL[..remaining.min(FILL.len())]);
    }
    value.truncate(width);
    value
}

fn cjk_line(generation: usize, row: usize, mixed: bool, unique_cjk: usize) -> Vec<u8> {
    let cjk_count = if mixed { 25 } else { 49 };
    let cjk = (0..cjk_count)
        .map(|column| {
            let unique_cjk = unique_cjk.min(18_000);
            let frame_index = (row * 49 + column) % unique_cjk;
            let index = generation * unique_cjk + frame_index;
            char::from_u32(0x4e00 + (index % 18_000) as u32).unwrap()
        })
        .collect::<String>();
    if !mixed {
        return cjk.into_bytes();
    }
    let ascii = pad_ascii(
        format!(" pytest::case_{row:02}_{generation:02} FAILED "),
        48,
    );
    let split = cjk
        .char_indices()
        .nth(cjk_count / 2)
        .map_or(cjk.len(), |(index, _)| index);
    format!(
        "\x1b[1;38;5;203m{}\x1b[22;38;5;81m{}\x1b[38;5;214m{}\x1b[0m",
        &cjk[..split],
        &cjk[split..],
        ascii,
    )
    .into_bytes()
}

fn full_repaint(lines: &[Vec<u8>]) -> Vec<u8> {
    full_repaint_rotated(lines, 0)
}

fn full_repaint_rotated(lines: &[Vec<u8>], offset: usize) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(b"\x1b[?2026h\x1b[H");
    for row in 0..lines.len() {
        payload.extend_from_slice(b"\x1b[2K");
        payload.extend_from_slice(&lines[(offset + row) % lines.len()]);
        if row + 1 != lines.len() {
            payload.extend_from_slice(b"\r\n");
        }
    }
    payload.extend_from_slice(b"\x1b[?2026l");
    payload
}

fn scroll_up(bottom_line: &[u8]) -> Vec<u8> {
    let mut payload = b"\x1b[?2026h\x1b[S\x1b[50;1H\x1b[2K".to_vec();
    payload.extend_from_slice(bottom_line);
    payload.extend_from_slice(b"\x1b[?2026l");
    payload
}

fn scroll_down(top_line: &[u8]) -> Vec<u8> {
    let mut payload = b"\x1b[?2026h\x1b[T\x1b[1;1H\x1b[2K".to_vec();
    payload.extend_from_slice(top_line);
    payload.extend_from_slice(b"\x1b[?2026l");
    payload
}
