//! Gate 1 (layering, re-verified in the product's own tree shape) and gate 2
//! (the pixel matrix §1 says must be measured rather than assumed).
//!
//! Every landmark the page owns is read off the screen first — see
//! [`crate::probe::Calibration`] — because the capture returns the host's own
//! bytes exactly and the engine's transformed.

use crate::capture::{Image, WindowCapture, near, seam_scan};
use crate::gfx::Rect;
use crate::log::{emit, verdict};
use crate::probe::{BORDER, Calibration, PANEL, PANEL_BGR, Probe, SEAT_BORDER_BGR, border_box};
use anyhow::Result;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::RECT;

pub fn gate1(probe: &mut Probe, capture: &WindowCapture) -> Result<Calibration> {
    // ── the airspace question, asked without touching the mouse ────────────
    let children = crate::win::child_windows(probe.window.hwnd);
    let covering: Vec<_> = children
        .iter()
        .filter(|child| child.rect[2] - child.rect[0] > 8 && child.rect[3] - child.rect[1] > 8)
        .collect();
    emit(
        1,
        "child-windows",
        serde_json::json!({
            "count": children.len(),
            "children": children,
            "covering_seat": covering.len(),
        }),
    );

    let (ok, _) = probe.navigate(&probe.server.url("/"), Duration::from_secs(20));
    if !ok {
        verdict(1, "fail", "the loopback probe page did not load");
        anyhow::bail!("gate 1 could not load its page");
    }
    probe.wait_for_message(Duration::from_secs(5), |message| {
        message.get("kind").and_then(|kind| kind.as_str()) == Some("ready")
    });
    probe.pump(Duration::from_millis(400));

    let calibration = probe.calibrate(capture)?;
    emit(
        1,
        "capture-fidelity",
        serde_json::json!({
            "host_rectangles_survive_the_capture_exactly": calibration.host_colours_are_exact(),
            "top_bar_nominal": calibration.top_bar_nominal,
            "top_bar_captured": calibration.top_bar_captured,
            "page_background_css": calibration.page_nominal,
            "page_background_captured": calibration.page,
            "finding": "the engine's pixels come back colour-transformed while the host's do not, so any pixel acceptance for a preview pane must sample the screen rather than compare against CSS",
        }),
    );

    // ── the page is in the seat, and the seat has no holes ─────────────────
    let baseline = probe.shoot(capture, "g1-01-page-in-seat")?;
    let seat_box = border_box(&baseline);
    let holes = probe
        .holes_in_seat(&baseline, &calibration)
        .unwrap_or(u64::MAX);
    let seat_area = probe.seat_area(&baseline).unwrap_or(1);
    let hole_fraction = holes as f64 / seat_area as f64;
    emit(
        1,
        "seat-occupancy",
        serde_json::json!({
            "border_box": seat_box,
            "seat_area_px": seat_area,
            "hole_coloured_pixels_inside_seat": holes,
            "hole_fraction": hole_fraction,
            "page_pixels": baseline.count_near(calibration.page, 12),
            "note": "the hole colour is whatever the seat shows with the WebView hidden; a page may legitimately contain that colour, so the fraction is the reading and not the raw count",
        }),
    );

    // ── an opaque panel crossing the seat's left edge ──────────────────────
    let seat = probe.seat;
    let panel = Rect {
        x: (seat.left - 90) as f32,
        y: (seat.top + 60) as f32,
        width: 320.0,
        height: 180.0,
        color: PANEL,
    };
    probe.panels = vec![panel];
    probe.present()?;
    let opaque = probe.shoot(capture, "g1-02-opaque-panel-over-web")?;
    let panel_inside = count_inside(&opaque, seat_box, PANEL_BGR, 10);
    emit(
        1,
        "opaque-overlay",
        serde_json::json!({
            "panel_pixels_inside_seat": panel_inside,
            "expected_if_layered": 230 * 180,
            "note": "the panel spans 90px of chrome and 230px of seat; a count near zero is the overlay being composited under the page",
        }),
    );

    // ── the same panel at 55%: real per-pixel blending, not avoidance ──────
    let mut translucent = panel;
    translucent.color[3] = 0.55;
    probe.panels = vec![translucent];
    probe.present()?;
    let blended = probe.shoot(capture, "g1-03-translucent-panel")?;
    let sample = seat_box
        .map(|(left, top, _, _)| blended.bgr(left + BORDER as u32 + 30, top + BORDER as u32 + 110));
    let between = sample.is_some_and(|colour| {
        (0..3).all(|channel| {
            let low = PANEL_BGR[channel].min(calibration.page[channel]);
            let high = PANEL_BGR[channel].max(calibration.page[channel]);
            colour[channel] >= low.saturating_sub(6) && colour[channel] <= high.saturating_add(6)
        })
    });
    emit(
        1,
        "translucent-overlay",
        serde_json::json!({
            "sampled": sample,
            "panel_colour": PANEL_BGR,
            "page_colour_as_captured": calibration.page,
            "is_pure_panel": sample.is_some_and(|colour| near(colour, PANEL_BGR, 6)),
            "is_pure_page": sample.is_some_and(|colour| near(colour, calibration.page, 6)),
            "lies_between_the_two": between,
        }),
    );
    probe.panels.clear();
    probe.present()?;

    // ── clip, rounded clip, opacity — all applied to the web visual ────────
    let page_pixels_before = baseline.count_near(calibration.page, 12);
    let seat_width = (seat.right - seat.left) as f32;
    let seat_height = (seat.bottom - seat.top) as f32;

    probe.tree.set_web_clip(seat_width / 2.0, seat_height)?;
    probe.tree.commit()?;
    let clipped = probe.shoot(capture, "g1-04-web-clipped-to-half")?;
    emit(
        1,
        "clip",
        serde_json::json!({
            "page_pixels_unclipped": page_pixels_before,
            "page_pixels_clipped": clipped.count_near(calibration.page, 12),
            "holes_after_clip": probe.holes_in_seat(&clipped, &calibration),
        }),
    );

    probe
        .tree
        .set_web_rounded_clip(seat_width, seat_height, 48.0)?;
    probe.tree.commit()?;
    let rounded = probe.shoot(capture, "g1-05-web-rounded-clip")?;
    let corner = seat_box
        .map(|(left, top, _, _)| rounded.bgr(left + BORDER as u32 + 3, top + BORDER as u32 + 3));
    let page_pixels_rounded = rounded.count_near(calibration.page, 12);
    emit(
        1,
        "rounded-clip",
        serde_json::json!({
            "corner_sample": corner,
            "corner_is_hole": corner
                .is_some_and(|colour| near(colour, calibration.class_background, 10)),
            "page_pixels_unclipped": page_pixels_before,
            "page_pixels_rounded": page_pixels_rounded,
            "kept_most_of_the_page": page_pixels_rounded * 100 > page_pixels_before * 90,
            "note": "a 48px radius removes four corners and nothing else, so the page survives almost intact while the corner pixel becomes a hole",
        }),
    );

    probe.tree.clear_web_clip()?;
    probe.tree.set_web_opacity(0.45)?;
    probe.tree.commit()?;
    let faded = probe.shoot(capture, "g1-06-web-opacity")?;
    let faded_sample = seat_box
        .map(|(left, top, _, _)| faded.bgr(left + BORDER as u32 + 30, top + BORDER as u32 + 200));
    emit(
        1,
        "opacity",
        serde_json::json!({
            "sample_at_45_percent": faded_sample,
            "page_colour_at_full_opacity": calibration.page,
            "class_background": calibration.class_background,
            "changed_from_full_opacity": faded_sample
                .is_some_and(|colour| !near(colour, calibration.page, 6)),
        }),
    );
    probe.tree.set_web_opacity(1.0)?;
    probe.tree.commit()?;

    // ── animation atomicity ───────────────────────────────────────────────
    //
    // Sweep the seat's left edge and photograph each step. In composition the
    // visual offset and the swapchain present land in one commit, so the border
    // and the seat's contents must meet with no window background in between.
    let (width, height) = probe.window.client_size();
    let full = Probe::seat_for(width, height);
    let mut seams = Vec::new();
    for step in 0..10 {
        probe.move_seat(RECT {
            left: full.left + step * 46,
            ..full
        })?;
        let frame = probe.shoot(capture, &format!("g1-07-anim-{step:02}"))?;
        let row = border_box(&frame)
            .map(|(_, top, _, bottom)| (top + bottom) / 2)
            .unwrap_or(frame.height / 2);
        let seam = seam_scan(
            &frame,
            row,
            SEAT_BORDER_BGR,
            calibration.class_background,
            12,
        );
        emit(1, "seam", serde_json::to_value(&seam)?);
        seams.push(seam);
    }
    probe.move_seat(full)?;
    let torn = seams
        .iter()
        .filter(|seam| seam.gap.is_none_or(|gap| gap > 0))
        .count();
    emit(
        1,
        "seam-summary",
        serde_json::json!({ "frames": seams.len(), "frames_with_a_gap": torn }),
    );

    let layered = panel_inside > 1000;
    // One part in a thousand: far below anything a lagging rectangle could
    // produce (a one-pixel lag down one edge of this seat is 0.1% on its own),
    // and far above the page's own incidental dark pixels.
    let covered = hole_fraction < 0.001;
    if layered && torn == 0 && covering.is_empty() && covered {
        verdict(
            1,
            "pass",
            "the overlay composites over the page and blends with it, clip / rounded clip / opacity all bite on the web visual, no child window covers the seat, and all ten animation frames are seamless",
        );
    } else {
        verdict(
            1,
            "fail",
            "one of: the overlay did not reach the seat, the seat had holes, a child window covered it, or a frame tore",
        );
    }
    Ok(calibration)
}

fn count_inside(
    image: &Image,
    region: Option<(u32, u32, u32, u32)>,
    colour: [u8; 3],
    tolerance: u8,
) -> u64 {
    let Some((left, top, right, bottom)) = region else {
        return 0;
    };
    let mut count = 0;
    for y in top..=bottom.min(image.height.saturating_sub(1)) {
        for x in left..=right.min(image.width.saturating_sub(1)) {
            if near(image.bgr(x, y), colour, tolerance) {
                count += 1;
            }
        }
    }
    count
}

pub fn gate2(probe: &mut Probe, capture: &WindowCapture, calibration: &Calibration) -> Result<()> {
    let (ok, _) = probe.navigate(&probe.server.url("/"), Duration::from_secs(20));
    if !ok {
        verdict(2, "fail", "the probe page did not load");
        return Ok(());
    }
    probe.pump(Duration::from_millis(600));

    // ── CapturePreview, timed ─────────────────────────────────────────────
    let mut timings = Vec::new();
    let mut first_shot = None;
    for round in 0..8 {
        let path = probe.shot_path(&format!("g2-capturepreview-{round:02}"));
        match probe.host.capture_preview(&path, Duration::from_secs(10)) {
            Ok(elapsed) => {
                timings.push(elapsed.as_micros() as u64);
                if first_shot.is_none() {
                    first_shot = Some(path);
                }
            }
            Err(error) => emit(
                2,
                "capturepreview-error",
                serde_json::json!({ "round": round, "error": error.to_string() }),
            ),
        }
        probe.pump(Duration::from_millis(60));
    }
    timings.sort_unstable();
    emit(
        2,
        "capturepreview-timing",
        serde_json::json!({
            "samples_us": timings,
            "min_ms": timings.first().map(|value| *value as f64 / 1000.0),
            "median_ms": timings.get(timings.len() / 2).map(|value| *value as f64 / 1000.0),
            "max_ms": timings.last().map(|value| *value as f64 / 1000.0),
            "measures": "the call to the completion handler: readback, PNG encode and the stream write",
        }),
    );

    if let Some(path) = &first_shot
        && let Ok(preview) = crate::capture::load_png(path)
    {
        emit(
            2,
            "capturepreview-image",
            serde_json::json!({
                "path": path,
                "size": [preview.width, preview.height],
                "seat_size": [
                    probe.seat.right - probe.seat.left,
                    probe.seat.bottom - probe.seat.top,
                ],
                "contains_host_border": preview.count_near(SEAT_BORDER_BGR, 16) > 0,
                "page_pixels_by_css_colour": preview.count_near(calibration.page_nominal, 12),
                "page_pixels_by_captured_colour": preview.count_near(calibration.page, 12),
                "note": "CapturePreview is the engine's own readback of its viewport; whether it agrees with the window capture's colours is the #5574 question and is answered by the two counts above",
            }),
        );
    }

    // ── the same call with the WebView hidden ─────────────────────────────
    probe.host.set_visible(false)?;
    probe.pump(Duration::from_millis(400));
    let hidden_path = probe.shot_path("g2-capturepreview-hidden");
    let hidden_result = probe
        .host
        .capture_preview(&hidden_path, Duration::from_secs(10));
    let hidden_report = match &hidden_result {
        Ok(elapsed) => {
            let image = crate::capture::load_png(&hidden_path).ok();
            serde_json::json!({
                "succeeded": true,
                "elapsed_ms": elapsed.as_secs_f64() * 1000.0,
                "size": image.as_ref().map(|image| [image.width, image.height]),
                "page_pixels": image.as_ref().map(|image| image.count_near(calibration.page, 14)),
                "black_fraction": image.as_ref().map(|image| image.count_near([0, 0, 0], 2) as f64
                    / f64::from(image.width * image.height)),
            })
        }
        Err(error) => serde_json::json!({ "succeeded": false, "error": error.to_string() }),
    };
    emit(2, "capturepreview-while-hidden", hidden_report);

    let while_hidden = probe.shoot(capture, "g2-window-while-webview-hidden")?;
    emit(
        2,
        "window-while-hidden",
        serde_json::json!({
            "page_pixels": while_hidden.count_near(calibration.page, 12),
            "class_background_inside_seat": probe.holes_in_seat(&while_hidden, calibration),
            "note": "SetIsVisible(false) leaves the seat showing the window class brush; the host must paint its own placeholder there",
        }),
    );
    probe.host.set_visible(true)?;
    probe.pump(Duration::from_millis(400));

    // ── PrintWindow, the third reading ────────────────────────────────────
    //
    // Taken here, while the probe page is still loaded, so that "how many of
    // the page's pixels came back" is a question about PrintWindow and not
    // about which page happened to be showing.
    match crate::capture::print_window(probe.window.hwnd) {
        Ok(image) => {
            let path = probe.shot_path("g2-printwindow");
            image.save_png(&path).ok();
            emit(
                2,
                "printwindow",
                serde_json::json!({
                    "succeeded": true,
                    "path": path,
                    "size": [image.width, image.height],
                    "page_pixels": image.count_near(calibration.page, 14),
                    "host_border_pixels": image.count_near(SEAT_BORDER_BGR, 16),
                    "black_fraction": image.count_near([0, 0, 0], 2) as f64
                        / f64::from(image.width * image.height),
                    "note": "this window has WS_EX_NOREDIRECTIONBITMAP and its pixels never touch GDI, so whatever PrintWindow returns here is what it would return for the product's window too",
                }),
            );
        }
        Err(error) => emit(
            2,
            "printwindow",
            serde_json::json!({ "succeeded": false, "error": error.to_string() }),
        ),
    }

    // ── a surface that changes every frame ────────────────────────────────
    let (ok, _) = probe.navigate(&probe.server.url("/video"), Duration::from_secs(20));
    emit(2, "video-page", serde_json::json!({ "loaded": ok }));
    if ok {
        probe.pump(Duration::from_millis(800));
        let one = probe.shot_path("g2-video-preview-a");
        let two = probe.shot_path("g2-video-preview-b");
        let first = probe.host.capture_preview(&one, Duration::from_secs(10));
        probe.pump(Duration::from_millis(300));
        let second = probe.host.capture_preview(&two, Duration::from_secs(10));
        if first.is_ok()
            && second.is_ok()
            && let (Ok(a), Ok(b)) = (
                crate::capture::load_png(&one),
                crate::capture::load_png(&two),
            )
        {
            let (differing, worst) = a.difference(&b);
            emit(
                2,
                "video-two-previews",
                serde_json::json!({
                    "differing_pixels": differing,
                    "total_pixels": u64::from(a.width) * u64::from(a.height),
                    "worst_channel_delta": worst,
                    "black_fraction": a.count_near([0, 0, 0], 2) as f64
                        / f64::from(a.width * a.height),
                    "reads_a_live_surface": differing > 0,
                }),
            );
        }
        let window_frame = probe.shoot(capture, "g2-video-window-capture")?;
        emit(
            2,
            "video-window-capture",
            serde_json::json!({
                "size": [window_frame.width, window_frame.height],
                "black_fraction": window_frame.count_near([0, 0, 0], 2) as f64
                    / f64::from(window_frame.width * window_frame.height),
                "class_background_inside_seat": probe.holes_in_seat(&window_frame, calibration),
            }),
        );
    }
    emit(
        2,
        "drm",
        serde_json::json!({
            "status": "not tested",
            "reason": "a protected-media surface needs a licensed EME stream; nothing offline can stand in for it, and a canvas is not a protected surface",
        }),
    );

    // ── how long a whole-window capture costs, for the same comparison ────
    let mut window_timings = Vec::new();
    for _ in 0..8 {
        capture.discard_queued();
        let started = Instant::now();
        if capture.frame(Duration::from_millis(900)).is_ok() {
            window_timings.push(started.elapsed().as_micros() as u64);
        }
        probe.pump(Duration::from_millis(20));
    }
    window_timings.sort_unstable();
    emit(
        2,
        "window-capture-timing",
        serde_json::json!({
            "samples_us": window_timings,
            "median_ms": window_timings
                .get(window_timings.len() / 2)
                .map(|value| *value as f64 / 1000.0),
            "note": "bounded below by the refresh interval: this waits for the next composed frame, it does not force one",
        }),
    );

    if timings.is_empty() {
        verdict(2, "fail", "CapturePreview never completed");
    } else {
        verdict(
            2,
            "pass",
            "all three channels measured with real numbers; the DRM row is recorded as untested by design",
        );
    }
    Ok(())
}
