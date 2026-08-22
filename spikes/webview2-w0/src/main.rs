//! W0′ — the ten gates of `docs/plans/web-preview/plan.md` §2, each one asked of
//! a real WebView2 in a real DirectComposition tree, each one answering with a
//! line of JSON on stdout.
//!
//! ```text
//! cargo run --release -- --gates all --shots <dir>
//! cargo run --release -- --gates 1,2 --hold        # leave the window up
//! cargo run --release -- --scenario runtime-missing
//! ```
//!
//! This is an independent workspace. It is not a member of the product's, no
//! product crate depends on it, and it is `unsafe_code = "allow"` because
//! DirectComposition, WebView2, UI Automation and Windows.Graphics.Capture have
//! no safe surface at all.

mod bindings;
mod capture;
mod dataobject;
mod dcomp;
mod gates_input;
mod gates_pixels;
mod gates_policy;
mod gates_system;
mod gates_w0p;
mod gfx;
mod host;
mod inject;
mod log;
mod machine;
mod policy;
mod probe;
mod procs;
mod server;
mod uia;
mod win;

use anyhow::Result;
use host::BoundsOrigin;
use std::path::PathBuf;
use std::time::Duration;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

struct Arguments {
    gates: Vec<u32>,
    shots: PathBuf,
    scenario: Option<String>,
    hold: bool,
    origin: BoundsOrigin,
}

fn parse_arguments() -> Arguments {
    let mut gates = Vec::new();
    let mut shots = std::env::temp_dir().join("w0-shots");
    let mut scenario = None;
    let mut hold = false;
    let mut origin = BoundsOrigin::AtZero;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--gates" => {
                let value = arguments.next().unwrap_or_default();
                gates = if value == "all" {
                    (1..=10).collect()
                } else {
                    value
                        .split(',')
                        .filter_map(|part| part.trim().parse().ok())
                        .collect()
                };
            }
            "--shots" => {
                if let Some(value) = arguments.next() {
                    shots = PathBuf::from(value);
                }
            }
            "--scenario" => scenario = arguments.next(),
            "--hold" => hold = true,
            "--bounds-at-seat" => origin = BoundsOrigin::AtSeat,
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    if gates.is_empty() && scenario.is_none() {
        gates = (1..=10).collect();
    }
    Arguments {
        gates,
        shots,
        scenario,
        hold,
        origin,
    }
}

fn main() {
    if let Err(error) = run() {
        // Same reason as the panic hook: a failure that only reaches stderr is
        // a failure the evidence file cannot show.
        log::emit(
            0,
            "fatal",
            serde_json::json!({ "error": format!("{error:#}") }),
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // Apartment-threaded, because that is what WebView2, DirectComposition and
    // UI Automation all expect of the thread that owns a window.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
    let arguments = parse_arguments();

    // A probe whose stdout is inherited by whatever launched it can die without
    // leaving a word behind — which is exactly what one gate did, exiting after
    // two log lines with the reason going wherever unredirected stderr goes.
    // Panics belong in the evidence file with everything else.
    std::panic::set_hook(Box::new(|info| {
        log::emit(
            0,
            "panic",
            serde_json::json!({
                "message": info.to_string(),
                "location": info.location().map(|at| format!("{}:{}", at.file(), at.line())),
            }),
        );
    }));

    if let Some(scenario) = &arguments.scenario {
        return run_scenario(scenario);
    }

    log::emit(
        0,
        "start",
        serde_json::json!({
            "gates": arguments.gates,
            "shots": arguments.shots,
            "bounds_origin": format!("{:?}", arguments.origin),
            "runtime_version": host::runtime_version(),
            "runtime_registry": host::runtime_registry_claims(),
            "bindings_transcribed": bindings::CHORDS.len(),
            "bindings_total": bindings::LEN,
        }),
    );

    let mut probe = probe::Probe::start(arguments.shots.clone(), arguments.origin)?;
    probe.pump(Duration::from_millis(400));

    // The window capture is opened once: standing a session up per frame would
    // cost more than the frames being measured.
    let capture = match capture::WindowCapture::start(probe.window.hwnd) {
        Ok(capture) => Some(capture),
        Err(error) => {
            log::emit(
                0,
                "window-capture-unavailable",
                serde_json::json!({ "error": error.to_string() }),
            );
            None
        }
    };

    // Gate 1 reads the landmarks off the screen; gate 2 needs the same reading,
    // and running gate 2 alone re-takes it rather than guessing.
    let mut calibration = None;
    for gate in &arguments.gates {
        let result = match (gate, &capture) {
            (1, Some(capture)) => gates_pixels::gate1(&mut probe, capture).map(|reading| {
                calibration = Some(reading);
            }),
            (2, Some(capture)) => {
                let reading = match calibration {
                    Some(reading) => Ok(reading),
                    None => {
                        probe.navigate(&probe.server.url("/"), Duration::from_secs(20));
                        probe.pump(Duration::from_millis(400));
                        probe.calibrate(capture)
                    }
                };
                reading.and_then(|reading| gates_pixels::gate2(&mut probe, capture, &reading))
            }
            (1 | 2, None) => {
                log::verdict(
                    *gate,
                    "blocked",
                    "Windows.Graphics.Capture is unavailable on this machine, so no pixel could be read",
                );
                Ok(())
            }
            (3, _) => gates_input::gate3(&mut probe),
            (4, _) => gates_input::gate4(&mut probe),
            (5, _) => gates_input::gate5(&mut probe),
            (6, _) => gates_system::gate6(&mut probe),
            (7, _) => gates_system::gate7(&mut probe),
            (8, _) => gates_system::gate8(&mut probe),
            (9, _) => gates_policy::gate9(&mut probe),
            (10, _) => gates_policy::gate10(&mut probe),
            _ => Ok(()),
        };
        if let Err(error) = result {
            log::emit(
                *gate,
                "gate-error",
                serde_json::json!({ "error": format!("{error:#}") }),
            );
            log::verdict(*gate, "fail", "the gate raised an error; see gate-error");
        }
    }

    if arguments.hold {
        log::note(0, "holding the window open; close it to finish");
        loop {
            let log = win::pump_for(Duration::from_millis(50), |_| {});
            if log.quit {
                break;
            }
        }
    }

    drop(capture);
    probe.shutdown()?;
    log::emit(0, "done", serde_json::json!({}));
    // **Why this does not fall off the end of `main`.** Every reading is
    // written by the time `done` is logged, and what remains is COM teardown:
    // a free-threaded capture frame pool, a wgpu device, a DirectComposition
    // device and a WebView2 environment, all releasing on one apartment thread
    // that is no longer pumping messages. Measured on this machine, that hangs
    // — the first run of this probe reached `done` in 16 seconds and was still
    // resident five minutes later, holding its own binary open against the next
    // build. The evidence is complete and flushed; the process says so and
    // leaves. Anything that *needed* an orderly release has already had it:
    // `Probe::shutdown` closes the controller and waits for
    // `BrowserProcessExited` before removing the user data folder.
    std::io::Write::flush(&mut std::io::stdout()).ok();
    leave_now();
}

/// Stop being a process, without asking the loader's permission.
///
/// `std::process::exit` is not enough here and the re-verification run proved
/// it: eight probe processes, one per gate group, each having written its
/// `done` line and called `exit(0)`, were **still resident** afterwards holding
/// their own binary and their user data folder open. `exit` runs the C
/// at-exit chain and then `DLL_PROCESS_DETACH` for every loaded module, and the
/// WebView2 loader's teardown on an apartment thread that has stopped pumping
/// is where those eight stopped. `TerminateProcess` on ourselves runs none of
/// it. Everything that needed an orderly release — the controller closed, the
/// browser waited for, the user data folder removed — already happened in
/// `Probe::shutdown` above.
fn leave_now() -> ! {
    use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
    // Unreachable in practice; the compiler does not know that.
    std::process::exit(0);
}

/// Scenarios are the things that cannot share a process with the main run.
fn run_scenario(scenario: &str) -> Result<()> {
    match scenario {
        // Gate 7's missing-runtime row. The parent sets
        // `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` to a folder that does not exist,
        // which is the only way to simulate a machine without the runtime
        // without uninstalling it.
        "runtime-missing" => {
            let api = host::runtime_version();
            let registry = host::runtime_registry_claims();
            let folder = std::env::temp_dir().join("w0-missing-udf");
            let started = std::time::Instant::now();
            let creation = host::environment(&folder);
            log::emit(
                7,
                "runtime-missing-detail",
                serde_json::json!({
                    "GetAvailableCoreWebView2BrowserVersionString": match &api {
                        Ok(version) => serde_json::json!({ "ok": version }),
                        Err(error) => serde_json::json!({ "error": error }),
                    },
                    "registry_still_claims": registry,
                    "create_environment_error": creation.as_ref().err().map(|error| format!("{error:#}")),
                    "failed_within_ms": started.elapsed().as_millis(),
                    "synchronous": started.elapsed() < std::time::Duration::from_millis(500),
                }),
            );
            Ok(())
        }
        other => {
            log::note(0, &format!("no scenario named {other}"));
            Ok(())
        }
    }
}
