//! **F1a spike: can one live WebView2 page be moved from one window to another,
//! whole?**
//!
//! The question `plan.md`'s F1a asks and this answers on the machine, against
//! the code that ships: `bt_platform::WebHost::rehost`, `bt_platform::Compositor`
//! and nothing written twice. What the probe adds is the two real windows, the
//! message pump, the injections and the evidence.
//!
//! # What counts as an answer
//!
//! Not a screenshot of a page that looks the same — a page reloaded at the same
//! address looks the same. The page mints a **boot id** at parse time and puts
//! it, with a tick count, into `document.title` four times a second; the host
//! reads those through `DocumentTitleChanged`. So "it did not navigate and it
//! did not lose its heap" is answerable out of the host's own event log: same
//! boot id across the handoff, tick count still climbing, and not one
//! `NavigationStarting` in between.
//!
//! ```text
//! cargo run -- --out ..\..\docs\spikes\artifacts\webview-rehost
//! ```

mod win;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bt_platform::{
    Compositor, RehostOutcome, RehostSide, WebEvent, WebHost, WebMouseEvent, WebNavigationVerdict,
};

const SEAT_A: u64 = 11;
const SEAT_B: u64 = 22;
const PAGE_WIDTH: u32 = 680;
const PAGE_HEIGHT: u32 = 420;

fn say(line: impl AsRef<str>) {
    println!("BT_SPIKE_REHOST {}", line.as_ref());
}

struct Args {
    out: PathBuf,
    udf: PathBuf,
}

fn args() -> Args {
    let mut out = PathBuf::from("artifacts");
    let mut udf = std::env::temp_dir().join("spike-webview-rehost-udf");
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        match flag.as_str() {
            "--out" => out = PathBuf::from(argv.next().unwrap_or_default()),
            "--udf" => udf = PathBuf::from(argv.next().unwrap_or_default()),
            other => say(format!("ignoring unknown argument {other}")),
        }
    }
    Args { out, udf }
}

/// Take what the engine said and say it out loud.
///
/// Every event, on stderr-shaped lines with the same prefix, because the whole
/// answer to "did it navigate" is a sequence of events and a trace that only
/// kept the ones somebody thought to ask about would be a trace that could not
/// be re-read later.
///
/// The four-a-second title events are the exception: they are the page's pulse
/// and would drown the log, so they are counted rather than printed.
fn absorb(host: &WebHost, log: &mut Vec<WebEvent>) {
    for event in host.drain() {
        match &event {
            WebEvent::DocumentTitleChanged { .. } | WebEvent::CursorChanged { .. } => {}
            WebEvent::Captured { png } => say(format!(
                "event Captured {{ png: {} bytes }}",
                png.as_ref().map_or(0, Vec::len)
            )),
            other => say(format!("event {other:?}")),
        }
        log.push(event);
    }
}

/// One place the probe waits, so that every wait has the same shape and the same
/// honest failure: a deadline, a pump, and the events that arrived meanwhile.
fn wait_for(
    host: &WebHost,
    log: &mut Vec<WebEvent>,
    label: &str,
    limit: Duration,
    mut done: impl FnMut(&[WebEvent]) -> bool,
) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        win::pump();
        absorb(host, log);
        if done(log) {
            return true;
        }
        if Instant::now() >= deadline {
            say(format!("TIMEOUT waiting for {label} after {limit:?}"));
            return false;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

/// Keep pumping for a while, so the page's own clock runs.
fn settle(host: &WebHost, log: &mut Vec<WebEvent>, how_long: Duration) {
    let until = Instant::now() + how_long;
    while Instant::now() < until {
        win::pump();
        absorb(host, log);
        std::thread::sleep(Duration::from_millis(8));
    }
}

/// The last `boot=… tick=…` the page published, parsed.
fn last_title(log: &[WebEvent]) -> Option<(String, u64)> {
    log.iter().rev().find_map(|event| match event {
        WebEvent::DocumentTitleChanged { title } => {
            let boot = title.strip_prefix("boot=")?;
            let (boot, tick) = boot.split_once(" tick=")?;
            Some((boot.to_owned(), tick.parse().ok()?))
        }
        _ => None,
    })
}

fn navigations(log: &[WebEvent]) -> usize {
    log.iter()
        .filter(|event| {
            matches!(
                event,
                WebEvent::NavigationStarting { .. } | WebEvent::NavigationCompleted { .. }
            )
        })
        .count()
}

fn write_png(bytes: &[u8], path: &Path) {
    match std::fs::write(path, bytes) {
        Ok(()) => say(format!("wrote {}", path.display())),
        Err(error) => say(format!("could not write {}: {error}", path.display())),
    }
}

/// Ask the engine for a picture of the page and wait for it.
fn capture(host: &WebHost, log: &mut Vec<WebEvent>, path: &Path) {
    let before = log.len();
    if let Err(error) = host.capture_preview() {
        say(format!("CapturePreview refused: {error}"));
        return;
    }
    let arrived = wait_for(host, log, "CapturePreview", Duration::from_secs(5), |log| {
        log[before..]
            .iter()
            .any(|event| matches!(event, WebEvent::Captured { .. }))
    });
    if !arrived {
        return;
    }
    if let Some(WebEvent::Captured { png: Some(png) }) = log[before..]
        .iter()
        .rev()
        .find(|event| matches!(event, WebEvent::Captured { .. }))
    {
        write_png(png, path);
    }
}

#[allow(clippy::too_many_lines)]
fn main() {
    let args = args();
    if let Err(error) = std::fs::create_dir_all(&args.out) {
        say(format!("cannot make {}: {error}", args.out.display()));
        return;
    }
    let _ = std::fs::create_dir_all(&args.udf);
    say(format!("user data folder: {}", args.udf.display()));
    match bt_platform::webview2_runtime_version() {
        Ok(version) => say(format!("runtime: {version}")),
        Err(error) => {
            say(format!("no WebView2 runtime: {error}"));
            return;
        }
    }

    win::become_dpi_aware();
    if let Err(error) = win::enter_apartment() {
        return say(error);
    }
    if let Err(error) = win::register_class() {
        say(error);
        return;
    }
    let window_a = match win::create_window("spike A (source)", 40, 40, 760, 560) {
        Ok(hwnd) => hwnd,
        Err(error) => return say(error),
    };
    // **Put the target window on a different scale if this machine has one.**
    // The plan's gate 8 is a mixed-DPI tear-out, and the DPI ownership question
    // has no answer at all on one monitor: an engine that never had to notice a
    // scale change cannot be observed noticing one.
    let scales = win::monitors();
    for monitor in &scales {
        say(format!("monitor {monitor:?}"));
    }
    let source_dpi = win::dpi_of(window_a);
    let elsewhere = scales
        .iter()
        .find(|monitor| monitor.dpi != source_dpi)
        .copied();
    let (bx, by) = match elsewhere {
        Some(monitor) => (monitor.left + 40, monitor.top + 40),
        None => (840, 40),
    };
    let window_b = match win::create_window("spike B (target)", bx, by, 760, 560) {
        Ok(hwnd) => hwnd,
        Err(error) => return say(error),
    };
    say(format!(
        "window A dpi={source_dpi} · window B dpi={} · different scales: {}",
        win::dpi_of(window_b),
        win::dpi_of(window_b) != source_dpi
    ));
    let compositor_a = match Compositor::new(win::handle(window_a)) {
        Ok(compositor) => compositor,
        Err(error) => return say(error),
    };
    let compositor_b = match Compositor::new(win::handle(window_b)) {
        Ok(compositor) => compositor,
        Err(error) => return say(error),
    };

    let mut host = WebHost::new(Box::new(|_| WebNavigationVerdict::Proceed), Box::new(|| {}));
    let mut log: Vec<WebEvent> = Vec::new();

    // ── 1. Up, in window A ─────────────────────────────────────────────────
    if let Err(error) = host.request_environment(&args.udf, 1) {
        return say(format!("request_environment: {error}"));
    }
    if !wait_for(
        &host,
        &mut log,
        "environment",
        Duration::from_secs(30),
        |log| {
            log.iter()
                .any(|event| matches!(event, WebEvent::Environment { .. }))
        },
    ) {
        return;
    }
    if let Err(error) = host.request_controller(win::handle(window_a), 1) {
        return say(format!("request_controller: {error}"));
    }
    if !wait_for(
        &host,
        &mut log,
        "controller",
        Duration::from_secs(30),
        |log| {
            log.iter()
                .any(|event| matches!(event, WebEvent::Controller { .. }))
        },
    ) {
        return;
    }
    if let Err(error) = compositor_a.attach_web_visual(SEAT_A) {
        return say(error);
    }
    if let Err(error) = host.install(&compositor_a, SEAT_A) {
        return say(format!("install: {error}"));
    }
    let _ = host.set_size(PAGE_WIDTH, PAGE_HEIGHT);
    let _ = compositor_a.place_web_visual(
        SEAT_A,
        (30, 60),
        (0.0, 0.0, PAGE_WIDTH as f32, PAGE_HEIGHT as f32),
    );
    let _ = host.set_visible(true);
    let _ = compositor_a.commit();

    let page = std::path::absolute(Path::new("assets/page.html"))
        .unwrap_or_else(|_| PathBuf::from("assets/page.html"));
    let url = format!("file:///{}", page.display().to_string().replace('\\', "/"));
    say(format!("navigating to {url}"));
    if let Err(error) = host.navigate(&url) {
        return say(format!("navigate: {error}"));
    }
    if !wait_for(
        &host,
        &mut log,
        "first load",
        Duration::from_secs(30),
        |log| {
            log.iter()
                .any(|event| matches!(event, WebEvent::NavigationCompleted { .. }))
        },
    ) {
        return;
    }
    settle(&host, &mut log, Duration::from_millis(1500));
    let before = last_title(&log);
    say(format!(
        "in A: boot/tick = {before:?} · browser pid = {}",
        host.browser_process_id()
    ));
    match host.dpi_ownership() {
        Some(dpi) => say(format!(
            "DPI OWNER PROBE in A: ShouldDetectMonitorScaleChanges={} RasterizationScale={} BoundsMode=RAW_PIXELS:{}",
            dpi.detects_monitor_scale_changes,
            dpi.rasterization_scale,
            dpi.bounds_mode_is_raw_pixels
        )),
        None => say("DPI OWNER PROBE: no controller"),
    }
    say(format!(
        "children of A before: {:?}",
        win::children_of(window_a)
    ));
    say(format!(
        "children of B before: {:?}",
        win::children_of(window_b)
    ));
    capture(&host, &mut log, &args.out.join("01-in-window-a.png"));
    let _ = win::screenshot(window_a, &args.out.join("02-window-a-live.png"));

    // ── 2. Injection: a parent window that is not a window ─────────────────
    //
    // The one place on a live machine where a real refusal can be produced
    // rather than simulated. It fails at `ParentWindow`, which is precisely the
    // step whose compensation has to put the old root visual target back.
    let ghost = win::destroyed_handle();
    if let Err(error) = compositor_b.attach_web_visual(SEAT_B) {
        return say(error);
    }
    let refused = host.rehost(
        &RehostSide {
            compositor: &compositor_a,
            seat: SEAT_A,
            hwnd: win::handle(window_a),
        },
        &RehostSide {
            compositor: &compositor_b,
            seat: SEAT_B,
            hwnd: ghost,
        },
        (PAGE_WIDTH, PAGE_HEIGHT),
        true,
    );
    say(format!("INJECT dead parent HWND -> {refused:?}"));
    let _ = compositor_a.commit();
    settle(&host, &mut log, Duration::from_millis(1200));
    let compensated = last_title(&log);
    say(format!(
        "after the refused handoff: boot/tick = {compensated:?} · navigations so far = {}",
        navigations(&log)
    ));
    let _ = win::screenshot(window_a, &args.out.join("03-window-a-after-refusal.png"));

    // ── 3. The real handoff ────────────────────────────────────────────────
    let navigations_before = navigations(&log);
    let _ = compositor_b.place_web_visual(
        SEAT_B,
        (30, 60),
        (0.0, 0.0, PAGE_WIDTH as f32, PAGE_HEIGHT as f32),
    );
    let moved = host.rehost(
        &RehostSide {
            compositor: &compositor_a,
            seat: SEAT_A,
            hwnd: win::handle(window_a),
        },
        &RehostSide {
            compositor: &compositor_b,
            seat: SEAT_B,
            hwnd: win::handle(window_b),
        },
        (PAGE_WIDTH, PAGE_HEIGHT),
        true,
    );
    say(format!("REHOST A -> B: {moved:?}"));
    let landed = matches!(moved, RehostOutcome::Moved);
    if landed {
        let _ = compositor_a.detach_web_visual(SEAT_A);
        let _ = compositor_a.commit();
        let _ = compositor_b.commit();
    }
    settle(&host, &mut log, Duration::from_millis(1500));
    let after = last_title(&log);
    say(format!(
        "in B: boot/tick = {after:?} · navigations during the handoff = {}",
        navigations(&log) - navigations_before
    ));
    say(format!(
        "children of A after: {:?}",
        win::children_of(window_a)
    ));
    say(format!(
        "children of B after: {:?}",
        win::children_of(window_b)
    ));
    match host.dpi_ownership() {
        Some(dpi) => say(format!(
            "DPI OWNER PROBE in B: ShouldDetectMonitorScaleChanges={} RasterizationScale={} BoundsMode=RAW_PIXELS:{}",
            dpi.detects_monitor_scale_changes,
            dpi.rasterization_scale,
            dpi.bounds_mode_is_raw_pixels
        )),
        None => say("DPI OWNER PROBE in B: no controller"),
    }
    capture(&host, &mut log, &args.out.join("04-in-window-b.png"));
    let _ = win::screenshot(window_b, &args.out.join("05-window-b-live.png"));
    let _ = win::screenshot(window_a, &args.out.join("06-window-a-empty.png"));

    // ── 4. Input, in the window it arrived in ──────────────────────────────
    let _ = host.send_mouse(WebMouseEvent::Move, (120, 90), 0);
    let _ = host.send_mouse(WebMouseEvent::LeftDown, (120, 90), 1);
    let _ = host.send_mouse(WebMouseEvent::LeftUp, (120, 90), 0);
    let _ = host.focus_page();
    settle(&host, &mut log, Duration::from_millis(900));
    say(format!(
        "focus events after the move: got={} lost={}",
        log.iter()
            .filter(|e| matches!(e, WebEvent::GotFocus))
            .count(),
        log.iter()
            .filter(|e| matches!(e, WebEvent::LostFocus))
            .count()
    ));
    capture(&host, &mut log, &args.out.join("07-input-in-window-b.png"));

    // ── 5. The browser dies under the moved page ───────────────────────────
    //
    // The gate `plan.md` names: after the move, a browser-process failure must
    // rebuild **here**. `bt_app::webhost` owns the state machine that decides
    // that; what this proves on the machine is that the address the rebuild
    // would read is window B's, and that a controller built on it puts the page
    // back in window B.
    let pid = host.browser_process_id();
    say(format!("killing our own browser process {pid}"));
    kill(pid);
    let _ = wait_for(
        &host,
        &mut log,
        "process failure",
        Duration::from_secs(20),
        |log| {
            log.iter().any(|event| {
                matches!(
                    event,
                    WebEvent::ProcessFailed { .. } | WebEvent::BrowserProcessExited { .. }
                )
            })
        },
    );
    say(format!(
        "the seat's address at rebuild time is window B: hwnd={:?} seat={SEAT_B}",
        win::handle(window_b)
    ));
    host.close();
    bt_platform::forget_web_environment();
    if let Err(error) = host.request_environment(&args.udf, 2) {
        say(format!("rebuild request_environment: {error}"));
    }
    let mut rebuilt = Vec::new();
    if wait_for(
        &host,
        &mut rebuilt,
        "rebuild environment",
        Duration::from_secs(40),
        |log| {
            log.iter()
                .any(|event| matches!(event, WebEvent::Environment { .. }))
        },
    ) {
        let _ = host.request_controller(win::handle(window_b), 2);
        if wait_for(
            &host,
            &mut rebuilt,
            "rebuild controller",
            Duration::from_secs(40),
            |log| {
                log.iter()
                    .any(|event| matches!(event, WebEvent::Controller { .. }))
            },
        ) {
            match host.install(&compositor_b, SEAT_B) {
                Ok(()) => {
                    let _ = host.set_size(PAGE_WIDTH, PAGE_HEIGHT);
                    let _ = host.set_visible(true);
                    let _ = compositor_b.commit();
                    let _ = host.navigate(&url);
                    let _ = wait_for(
                        &host,
                        &mut rebuilt,
                        "rebuild load",
                        Duration::from_secs(30),
                        |log| {
                            log.iter()
                                .any(|e| matches!(e, WebEvent::NavigationCompleted { .. }))
                        },
                    );
                    settle(&host, &mut rebuilt, Duration::from_millis(1200));
                    say(format!(
                        "rebuilt in B: boot/tick = {:?} (a NEW boot id is correct here — the page was rebuilt, not moved)",
                        last_title(&rebuilt)
                    ));
                    let _ = win::screenshot(window_b, &args.out.join("08-rebuilt-in-window-b.png"));
                }
                Err(error) => say(format!("rebuild install: {error}")),
            }
        }
    }

    // ── 6. Down, and nothing left behind ───────────────────────────────────
    host.close();
    let _ = wait_for(
        &host,
        &mut rebuilt,
        "browser exit",
        Duration::from_secs(15),
        |log| {
            log.iter()
                .any(|event| matches!(event, WebEvent::BrowserProcessExited { .. }))
        },
    );
    win::close_window(window_a);
    win::close_window(window_b);
    win::pump();

    say(format!(
        "VERDICT moved={landed} · boot id before={:?} after={:?} · navigations during handoff={}",
        before.as_ref().map(|(boot, _)| boot),
        after.as_ref().map(|(boot, _)| boot),
        navigations(&log) - navigations_before
    ));
    say(format!(
        "TICKS before={:?} after={:?} (climbing across the handoff means the same document kept running)",
        before.map(|(_, tick)| tick),
        after.map(|(_, tick)| tick)
    ));
}

/// End **our own** browser process, and only ever that one: the pid comes from
/// `ICoreWebView2::BrowserProcessId` on the controller this probe built, over
/// the user data folder this probe made.
fn kill(pid: u32) {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    if pid == 0 {
        say("no browser process id to end");
        return;
    }
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) };
    match handle {
        Ok(handle) => {
            let _ = unsafe { TerminateProcess(handle, 1) };
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
        }
        Err(error) => say(format!("OpenProcess({pid}): {error}")),
    }
}
