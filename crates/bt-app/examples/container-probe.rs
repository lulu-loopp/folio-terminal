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
//! It is kept rather than deleted for the reason `test-assets/PROVENANCE.md`
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

use std::path::Path;
use std::time::{Duration, Instant};

use bt_platform::video::engine::Engine;

fn main() {
    for argument in std::env::args().skip(1) {
        let path = Path::new(&argument);
        print!("{} -> ", path.display());
        let mut engine = match Engine::open(path) {
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
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut frames = 0_u64;
        while Instant::now() < deadline && frames < 3 {
            if let Some(frame) = engine.frame() {
                frames += 1;
                println!("    frame {} {}x{}", frame.generation, frame.width, frame.height);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let state = engine.state();
        println!(
            "    frames={frames} position={:.3} error={:?}",
            state.position_secs, state.error
        );
        engine.shutdown();
    }
    bt_platform::video::shutdown_media_session();
}
