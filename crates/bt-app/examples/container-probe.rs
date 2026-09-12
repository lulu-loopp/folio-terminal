//! **Open each container named on the command line and report whether a frame
//! ever arrives** — the tool the playable matrix is built with (route B slice
//! ②, 2026-08-28; `docs/DESIGN.md` §7.44 ⑥).
//!
//! It exists because §7.42 ⑧ caught `IMFMediaEngine::CanPlayType` **under**
//! reporting what the platform will open: it answers `No` for `video/quicktime`,
//! `video/x-matroska`, `video/avi` and `video/x-ms-wmv`, and Media Foundation
//! opens all four. So `preview::VIDEO_EXTENSIONS` is not built by asking that
//! function — it is built by handing a real file to a real engine, which is what
//! this does.
//!
//! It is kept rather than deleted for the reason `tests/assets/PROVENANCE.md`
//! keeps the ffmpeg command beside every fixture: a row of that table is a
//! measurement, and a measurement nobody can repeat is a claim. Adding a row
//! means making a fixture and running this against it.
//!
//! ```text
//! cargo run --example container-probe -- clip.mkv clip.avi clip.wmv
//! ```
//!
//! **The answer is per machine and says so.** Six of the seven rows are
//! containers and codecs Windows ships; `.webm` needs the Store's VP9/WebMedia
//! extensions, and on a machine without them this probe is what shows the
//! `Unsupported` rather than a guess about it.
//!
//! # Why the gate is in the source rather than in the manifest (M1-10)
//!
//! Media Foundation is the thing being measured, so off Windows there is
//! nothing here to run — `Engine::wait_for_metadata` is one of the methods the
//! portable `video` module does not have and is not meant to grow until M4-4.
//! The gate is `#[cfg(windows)]` on each item with a portable `main` beside it,
//! and **not** `required-features` on a `[[example]]` table, because cargo's
//! features are resolved for the whole graph rather than per target: there is no
//! feature spelling that means "on Windows". A `required-features` example is
//! simply *skipped* by `--all-targets`, which would make `cargo check
//! --all-targets` on the Mac pass by not looking — and a target nobody compiles
//! is the thing this ticket exists to stop. With the gate here, both platforms
//! compile this file and each gets the half that is true of it.

#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use bt_platform::video::engine::Engine;

/// The probe is a Windows measurement and says so rather than pretending.
#[cfg(not(windows))]
fn main() {
    eprintln!(
        "container-probe is a Windows probe: it measures what Media Foundation opens, and this \
         platform has no such engine (macOS is M4-4/M4-5)."
    );
}

#[cfg(windows)]
fn main() {
    for argument in std::env::args().skip(1) {
        let path = Path::new(&argument);
        print!("{} -> ", path.display());
        // **What `Engine::open` costs the thread that calls it**, which is the
        // measurement the freeze of 2026-08-28 was found with: name the same
        // file twice on the command line and the first number is what a cold
        // process pays and the second is what a warm one does.
        let began = Instant::now();
        let opened = Engine::open(path);
        let open_cost = began.elapsed();
        print!("open={:.0}ms ", open_cost.as_secs_f64() * 1000.0);
        let mut engine = match opened {
            Ok(engine) => engine,
            Err(error) => {
                println!("open failed: {error:?}");
                continue;
            }
        };
        let ready = engine.wait_for_metadata(Duration::from_secs(5));
        let state = engine.state();
        println!(
            "ready={ready} size={:?} duration={:?} has_video={} error={:?}",
            state.natural_size, state.duration_secs, state.has_video, state.error
        );
        engine.play();
        // **Five whole seconds and not the first three frames.** A probe that
        // stopped at three could not tell a container that decodes from one that
        // decodes twice and stops, which is the question the freeze of
        // 2026-08-28 turned out to be about. The first three are still printed
        // one by one, because a row of the matrix is "a picture arrived" and a
        // reader wants to see the first ones arrive.
        let began = Instant::now();
        let deadline = began + Duration::from_secs(5);
        let mut frames = 0_u64;
        let mut last_at = began;
        let mut longest_gap = Duration::ZERO;
        while Instant::now() < deadline {
            if let Some(frame) = engine.frame() {
                frames += 1;
                let at = Instant::now();
                longest_gap = longest_gap.max(at.saturating_duration_since(last_at));
                last_at = at;
                if frames <= 3 {
                    println!(
                        "    frame {} {}x{}",
                        frame.generation, frame.width, frame.height
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        longest_gap = longest_gap.max(Instant::now().saturating_duration_since(last_at));
        let state = engine.state();
        println!(
            "    frames={frames} in {:.1}s longest_gap={:.0}ms position={:.3} playing={} error={:?}",
            began.elapsed().as_secs_f64(),
            longest_gap.as_secs_f64() * 1000.0,
            state.position_secs,
            state.playing,
            state.error
        );
        engine.shutdown();
    }
    bt_platform::video::shutdown_media_session();
}
