//! Temporary: open each container handed on the command line with the media
//! engine and report whether a frame ever arrives. Deleted before the branch is
//! reported; it exists to build the playable matrix off real opens rather than
//! off `CanPlayType`.

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
