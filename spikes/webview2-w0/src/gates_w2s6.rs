//! W2 slice ⑥ — what a **focus card** would have to pay for a picture of a page.
//!
//! Gate 2 of `plan.md` §2 already timed `CapturePreview` and wrote down one
//! number: 52.7 ms median. That number is a *latency*, measured from the call to
//! the completion handler, and slice ⑥'s whole problem is that it is being read
//! against a *budget* — `focus_thumb::FULL_BLAST_BUDGET_MS = 3.0` per frame at
//! 10 Hz. The two are not the same quantity, and no design can be chosen until
//! somebody has measured which of them the asking thread actually pays.
//!
//! So this gate asks four questions gate 2 did not:
//!
//! 1. **What does the ask cost the caller?** The synchronous half of
//!    `CapturePreview`, timed on its own, separately from the wait.
//! 2. **Does the target size change anything?** A card is 263 × 160 logical
//!    pixels. `CapturePreview` has no size parameter — it captures the viewport
//!    — so the only way to get a small picture is to have a small seat, which a
//!    card never has. The sweep says what the number does across four seat
//!    sizes, so that the design is chosen against the size a *pane* is.
//! 3. **What does the picture cost after it arrives?** Decode and resample to
//!    card size, on the thread that would be drawing.
//! 4. **Does a capture in flight disturb the frames beside it?** A render loop
//!    is run at vsync with and without captures being issued, and the two
//!    interval distributions are printed side by side.
//!
//! And it re-asks the one gate 2 already answered, at card scale, because it is
//! the fact the whole shape stands on: **a hidden WebView never completes a
//! capture**, and in a focus column every card but one is a background tab.

use crate::log::{emit, note, verdict};
use crate::probe::Probe;
use anyhow::Result;
use std::time::{Duration, Instant};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG, COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
};
use windows::Win32::Foundation::RECT;

/// The card body a projection is cut to today: `FOCUS_COLUMN_WIDTH_LOGICAL_PX`
/// 280 less its padding, and the tallest of the three card heights.
const CARD_WIDTH: u32 = 263;
const CARD_HEIGHT: u32 = 320;

/// How many rounds each size is asked for. Eight, the same count gate 2 used, so
/// the medians are comparable with the ones already in the evidence file.
const ROUNDS: usize = 8;

fn micros(samples: &[u64]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    serde_json::json!({
        "samples_us": sorted,
        "min_ms": sorted.first().map(|value| *value as f64 / 1000.0),
        "median_ms": sorted.get(sorted.len() / 2).map(|value| *value as f64 / 1000.0),
        "max_ms": sorted.last().map(|value| *value as f64 / 1000.0),
    })
}

/// One seat size, measured end to end: ask, wait, read back, decode, resample.
fn sweep_one_size(probe: &mut Probe, label: &str, seat: RECT) -> Result<Option<f64>> {
    probe.move_seat(seat)?;
    probe.pump(Duration::from_millis(500));
    let width = (seat.right - seat.left) as u32;
    let height = (seat.bottom - seat.top) as u32;

    let mut issue = Vec::new();
    let mut complete = Vec::new();
    let mut read_back = Vec::new();
    let mut bytes = Vec::new();
    let mut decoded_size = None;
    let mut decode = Vec::new();
    let mut resample = Vec::new();
    let mut errors = Vec::new();
    for _ in 0..ROUNDS {
        match probe
            .host
            .capture_preview_to_memory(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                Duration::from_secs(10),
            ) {
            Ok(timing) => {
                issue.push(timing.issue.as_micros() as u64);
                complete.push(timing.complete.as_micros() as u64);
                read_back.push(timing.read_back.as_micros() as u64);
                bytes.push(timing.bytes.len() as u64);
                // Decode and resample on **this** thread, because that is where
                // a window that wanted to draw the result would be doing it.
                let started = Instant::now();
                let image = image::load_from_memory_with_format(
                    &timing.bytes,
                    image::ImageFormat::Png,
                );
                let decoded = started.elapsed();
                match image {
                    Ok(image) => {
                        decode.push(decoded.as_micros() as u64);
                        decoded_size = Some([image.width(), image.height()]);
                        let started = Instant::now();
                        let small = image::imageops::resize(
                            &image.to_rgba8(),
                            CARD_WIDTH,
                            CARD_HEIGHT,
                            image::imageops::FilterType::Triangle,
                        );
                        resample.push(started.elapsed().as_micros() as u64);
                        std::hint::black_box(small.as_raw().len());
                    }
                    Err(error) => errors.push(error.to_string()),
                }
            }
            Err(error) => errors.push(error.to_string()),
        }
        probe.pump(Duration::from_millis(40));
    }

    let median_complete = {
        let mut sorted = complete.clone();
        sorted.sort_unstable();
        sorted.get(sorted.len() / 2).map(|value| *value as f64 / 1000.0)
    };
    emit(
        11,
        "capture-size-sweep",
        serde_json::json!({
            "size": label,
            "seat_px": [width, height],
            "decoded_px": decoded_size,
            "issue": micros(&issue),
            "complete": micros(&complete),
            "read_back": micros(&read_back),
            "decode": micros(&decode),
            "resample_to_card": micros(&resample),
            "png_bytes": bytes,
            "errors": errors,
            "note": "issue is the synchronous call — what the asking thread pays; complete is the latency gate 2 already recorded; decode and resample are what the answer costs after it arrives",
        }),
    );
    Ok(median_complete)
}

/// The same page, one size, asked for as JPEG instead of PNG.
fn jpeg_round(probe: &mut Probe) -> Result<()> {
    let mut complete = Vec::new();
    let mut bytes = Vec::new();
    let mut errors = Vec::new();
    for _ in 0..ROUNDS {
        match probe.host.capture_preview_to_memory(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG,
            Duration::from_secs(10),
        ) {
            Ok(timing) => {
                complete.push(timing.complete.as_micros() as u64);
                bytes.push(timing.bytes.len() as u64);
            }
            Err(error) => errors.push(error.to_string()),
        }
        probe.pump(Duration::from_millis(40));
    }
    emit(
        11,
        "capture-format-jpeg",
        serde_json::json!({
            "complete": micros(&complete),
            "jpeg_bytes": bytes,
            "errors": errors,
            "note": "the other format the API offers, in case the encode is where the latency lives",
        }),
    );
    Ok(())
}

/// What the frame loop is doing beside the captures.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Beside {
    /// Nothing. The baseline.
    Nothing,
    /// Ask, then **wait** for the answer before the frame ends — the naive
    /// arrangement, priced so that the report can say what it costs.
    Blocking,
    /// Ask, let the frame end, and pick the answer up on a later pump — the only
    /// arrangement a window could have.
    Pipelined,
}

/// Frames, with and without captures being issued between them.
///
/// The loop is the shape a window's is: draw, commit, pump. Under Fifo the
/// interval is the display's, so what the three runs are compared on is the
/// **tail** — a frame that missed its slot is a frame somebody saw stutter.
fn render_loop(probe: &mut Probe, frames: usize, beside: Beside) -> Result<serde_json::Value> {
    let mut intervals = Vec::new();
    let mut draws = Vec::new();
    let mut issues = Vec::new();
    let mut latencies = Vec::new();
    let mut collects = Vec::new();
    let mut completed = 0u32;
    let mut issued = 0u32;
    let mut in_flight: Option<crate::host::CaptureInFlight> = None;
    let mut last = Instant::now();
    for _ in 0..frames {
        let started = Instant::now();
        probe.present()?;
        draws.push(started.elapsed().as_micros() as u64);
        match beside {
            Beside::Nothing => {}
            Beside::Blocking => {
                issued += 1;
                if let Ok(timing) = probe.host.capture_preview_to_memory(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    Duration::from_secs(10),
                ) {
                    completed += 1;
                    issues.push(timing.issue.as_micros() as u64);
                    latencies.push(timing.complete.as_micros() as u64);
                }
            }
            Beside::Pipelined => {
                // One at a time: a second ask while the first is unanswered
                // would be a queue nobody is draining.
                match &in_flight {
                    Some(capture) => {
                        if let Some(latency) = capture.settled() {
                            let started = Instant::now();
                            let bytes = capture.bytes().map(|bytes| bytes.len()).unwrap_or(0);
                            collects.push(started.elapsed().as_micros() as u64);
                            std::hint::black_box(bytes);
                            latencies.push(latency.as_micros() as u64);
                            completed += 1;
                            in_flight = None;
                        }
                    }
                    None => {
                        if let Ok(capture) =
                            probe.host.begin_capture_preview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG)
                        {
                            issued += 1;
                            issues.push(capture.issue.as_micros() as u64);
                            in_flight = Some(capture);
                        }
                    }
                }
            }
        }
        probe.pump(Duration::from_millis(1));
        let now = Instant::now();
        intervals.push(now.duration_since(last).as_micros() as u64);
        last = now;
    }
    let over_20ms = intervals.iter().filter(|value| **value > 20_000).count();
    Ok(serde_json::json!({
        "frames": frames,
        "interval": micros(&intervals),
        "draw_and_commit": micros(&draws),
        "issue": micros(&issues),
        "latency": micros(&latencies),
        "collect_bytes": micros(&collects),
        "frames_over_20ms": over_20ms,
        "captures_issued": issued,
        "captures_completed": completed,
    }))
}

/// The whole gate.
pub fn gate_w2_slice6(probe: &mut Probe) -> Result<()> {
    let (ok, _) = probe.navigate(&probe.server.url("/"), Duration::from_secs(20));
    if !ok {
        verdict(11, "fail", "the probe page did not load");
        return Ok(());
    }
    probe.pump(Duration::from_millis(600));

    let (width, height) = probe.window.client_size();
    let full = Probe::seat_for(width, height);
    let origin_x = full.left;
    let origin_y = full.top;
    let sizes: [(&str, u32, u32); 4] = [
        ("pane-full", (full.right - full.left) as u32, (full.bottom - full.top) as u32),
        ("pane-half", 640, 400),
        ("card-tall", CARD_WIDTH, CARD_HEIGHT),
        ("card-short", CARD_WIDTH, 160),
    ];
    let mut medians = serde_json::Map::new();
    for (label, seat_width, seat_height) in sizes {
        let seat = RECT {
            left: origin_x,
            top: origin_y,
            right: origin_x + seat_width as i32,
            bottom: origin_y + seat_height as i32,
        };
        if let Some(median) = sweep_one_size(probe, label, seat)? {
            medians.insert(label.to_owned(), serde_json::json!(median));
        }
    }
    emit(
        11,
        "capture-medians-by-size",
        serde_json::Value::Object(medians),
    );

    // Back to a whole pane for everything that follows: that is the size a page
    // actually has when a card is drawn of it.
    probe.move_seat(full)?;
    probe.pump(Duration::from_millis(400));
    jpeg_round(probe)?;

    note(11, "render loop: baseline");
    let baseline = render_loop(probe, 180, Beside::Nothing)?;
    note(11, "render loop: asking and waiting inside the frame");
    let blocking = render_loop(probe, 180, Beside::Blocking)?;
    note(11, "render loop: asking and collecting on a later pump");
    let pipelined = render_loop(probe, 180, Beside::Pipelined)?;
    emit(
        11,
        "render-loop-interference",
        serde_json::json!({
            "baseline": baseline,
            "blocking": blocking,
            "pipelined": pipelined,
        }),
    );

    // The fact the whole shape rests on, asked again at card scale.
    probe.host.set_visible(false)?;
    probe.pump(Duration::from_millis(400));
    let started = Instant::now();
    let hidden = probe.host.capture_preview_to_memory(
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
        Duration::from_secs(5),
    );
    emit(
        11,
        "capture-while-hidden",
        serde_json::json!({
            "completed": hidden.is_ok(),
            "waited_ms": started.elapsed().as_millis(),
            "error": hidden.as_ref().err().map(std::string::ToString::to_string),
            "note": "a focus column shows one card per tab and only the active tab's page is on the glass, so this row decides how many cards can ever hold a fresh picture",
        }),
    );
    probe.host.set_visible(true)?;
    probe.pump(Duration::from_millis(200));

    verdict(
        11,
        "measured",
        "see capture-size-sweep, render-loop-interference and capture-while-hidden",
    );
    Ok(())
}
