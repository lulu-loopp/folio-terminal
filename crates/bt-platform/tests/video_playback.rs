//! **A video that is playing, asked of whichever engine this machine has**
//! (M4-5; `docs/DESIGN.md` §13.35).
//!
//! `video/engine.rs`'s own cases are Media Foundation's and never compile on a
//! Mac; `macos_player.rs`'s are AVFoundation's and never compile on Windows.
//! **This file is the one that is the same on both**, which is the whole claim
//! M4-5 makes — that a video plays, seeks, pauses and ends the same way on
//! either machine — and the only way to state a claim about two arms is to write
//! it once and run it twice. It is `tests/video_first_frame.rs`'s instrument one
//! ticket later.
//!
//! # Why this owns the process's main thread, and it is not AppKit's reason
//!
//! `harness = false`, like `macos_sheet.rs` and `macos_compose.rs` — and for a
//! reason none of them has. **Nothing here touches AppKit.** What it needs the
//! main thread for is that `AVPlayer` serializes its own state changes onto a
//! dispatch queue that its header says "by default … is the main queue", and
//! **only a live main run loop drains that queue**.
//!
//! Measured on the venue machine, 2026-09-12, with these cases under libtest:
//! `AVPlayerItem.status` stayed `Unknown` for fifteen seconds, `play` set the
//! rate and `currentTime` never moved off `0.000`, and not one picture arrived —
//! while every fact read off the **asset** (160×120, 5.000 s, has video, has
//! audio) was correct. A `#[test]` runs on a thread libtest spawned and libtest's
//! own main thread is parked in a join, so nothing on that process ever services
//! the main queue and an item can never finish becoming ready.
//!
//! So [`pump`] is this file's `sleep`: on macOS it is
//! `CFRunLoopRunInMode(kCFRunLoopDefaultMode, …)`, which drains the main queue
//! and returns, and everywhere else it is an ordinary sleep. Folio itself always
//! has that run loop — it is winit's — so this is a fact about a *test process*
//! and not about the product; §13.35 ⑤ is where it is written down and where
//! what it means for a busy main thread is weighed.
//!
//! Running the cases from one `main` also buys something libtest was taking
//! away: they run **in order and one at a time**, so the process-wide engine
//! ledger is a statement about one case rather than about eight.
//!
//! # The recordings, and what is known about them before anything plays
//!
//! `tests/assets/PROVENANCE.md` records how the fixtures were made: 160×120, a
//! fifth of a second of black and then one solid colour, about five frames a
//! second. The `.mp4` is **5.000 s** and the `.mov` is **3.000 s**, and the third
//! one used here — `folio-video-sound-test.mp4`, 3.000 s — is the only one of
//! the six with an **audio track**, which is why the case about sound names it
//! and the others do not.
//!
//! # What an agent cannot assert, said rather than skipped
//!
//! **Nothing in this file hears anything.** Whether a speaker made a noise is
//! not a thing a test process can read back, and a case that claimed it would be
//! a case that passes on a machine with the volume at zero, no output device, or
//! the audio path deleted. What is asserted instead is everything up to the
//! speaker: that the file has an audio track and the engine says so, that
//! `set_volume` and `set_muted` reach the engine and are read back off it, and
//! that the picture and the clock keep running while they do. §M4's acceptance
//! ③ — "it plays **with audible sound**" — is the owner's ear, and it is
//! performed by hand against `folio-video-sound-test.mp4`.
//!
//! **Every engine in this file is muted before it is played**, because these
//! cases run on the owner's own machine while the owner is working.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bt_platform::video::engine::{
    Engine, EngineError, EngineState, engines_outstanding, engines_shut_down, engines_started,
};
use bt_platform::video::first_frame;

/// Whether this machine has a playback engine at all. A third platform refuses
/// everything here, and that is the assertion rather than a skip — a build whose
/// refusal quietly became a panic is the failure this catches.
const HAS_ENGINE: bool = cfg!(any(windows, target_os = "macos"));

/// Both fixtures are this, and it is the pair a layout is solved from.
const NATIVE: (u32, u32) = (160, 120);

/// How long a case waits for something that ought to take milliseconds.
///
/// **Generous on purpose**, for the Windows arm's own reason: what is being
/// asserted is that frames arrive at all, and how many arrive in a second is a
/// fact about the machine the test is running on rather than about this lane.
const PATIENCE: Duration = Duration::from_secs(15);

/// **How far the still and the playing picture may sit apart, per channel, out
/// of 255.**
///
/// Forty, and the number is a decision rather than a shrug. The two pictures
/// reach a caller by two different roads *on the same machine* — the still is
/// drawn through a `CGBitmapContext` that names sRGB and is colour-matched into
/// it (§13.25 ③ measured that at up to 20 off the authored colour), and the
/// playing frame comes out of the player's own video output as `32BGRA`. This is
/// the gate over the one thing a reader would see: §7.42 ⑤ made the *rectangle*
/// the same for the still and the first played frame, and this makes the
/// *colour* the same, so that pressing ▶ does not make the picture jump. Forty
/// is wide enough for two roads through one colour pipeline and nowhere near the
/// defect it is for — a red and blue swapped between the two is ~180 off.
const STILL_TO_PLAYING: i32 = 40;

/// **This file's `sleep`, and on a Mac it is the whole reason the file owns the
/// main thread.**
///
/// See the module note. On macOS it runs the main run loop for `budget`, which
/// is what drains the main dispatch queue and therefore what lets an
/// `AVPlayerItem` finish becoming ready; everywhere else there is nothing to
/// drain and it is an ordinary sleep.
fn pump(budget: Duration) {
    #[cfg(target_os = "macos")]
    {
        use objc2_core_foundation::{CFRunLoop, kCFRunLoopDefaultMode};

        // SAFETY: a framework constant that lives for the process. The call
        // itself is safe in these bindings — it runs this thread's own run loop,
        // which is the main one, for at most `budget` and returns.
        let mode = unsafe { kCFRunLoopDefaultMode };
        CFRunLoop::run_in_mode(mode, budget.as_secs_f64(), false);
    }
    #[cfg(not(target_os = "macos"))]
    std::thread::sleep(budget);
}

/// One of the shipped recordings.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/assets")
        .join(name)
}

/// An engine on `name`, opened, muted and waited for. `None` on a platform that
/// has no decoder, which is the one place in this file that branches.
fn ready_engine(name: &str) -> Option<Engine> {
    let engine = match Engine::open(&fixture(name)) {
        Ok(engine) => engine,
        Err(error) => {
            // The only refusal a shipped recording may meet at this door is a
            // machine with no decoder at all; both real arms open a thread and
            // put any refusal in the state instead.
            assert_eq!(
                Some(error),
                (!HAS_ENGINE).then_some(EngineError::Unsupported),
                "{name} was refused at `open`"
            );
            return None;
        }
    };
    // Before anything is played: nothing this suite runs may reach the owner's
    // speakers.
    engine.set_muted(true);
    // Both halves, because the mute is a command the engine's own thread
    // applies and the state is what that thread last published: a fixture whose
    // metadata arrives in its constructor is `ready` on the first publish, one
    // turn before the mute has been applied, and a case that read the state on
    // that turn would see an engine that is about to be muted rather than one
    // that is.
    let ready = settle(&engine, |state| state.ready && state.muted);
    assert!(ready.ready, "{name} never loaded: {ready:?}");
    assert!(
        ready.muted,
        "{name} was told to be quiet and is not: {ready:?}"
    );
    Some(engine)
}

/// Spin until `settled` answers true, or [`PATIENCE`] runs out, and hand back
/// the last state either way — so a failing assertion prints what was actually
/// true rather than the fact that a wait ended.
fn settle(engine: &Engine, settled: impl Fn(EngineState) -> bool) -> EngineState {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let state = engine.state();
        if settled(state) || Instant::now() >= deadline {
            return state;
        }
        pump(Duration::from_millis(4));
    }
}

/// **Where a clock that has been told to stop comes to rest.**
///
/// Measured rather than assumed, and it is a measurement because of something
/// this file found on 2026-09-12: on the Windows arm `IsPaused` answers the
/// *request* and the pipeline behind it takes a moment to actually stop, which
/// on a loaded machine is a few hundred milliseconds of clock after `playing`
/// has already gone false. That is not a defect — a player that reported "still
/// playing" until its pipeline had wound down would leave a pressed pause button
/// lit — but it does mean the honest claim is "the clock **comes to rest**", not
/// "the flag and the clock change in the same instant".
fn at_rest(engine: &Engine) -> f64 {
    let deadline = Instant::now() + PATIENCE;
    let mut last = engine.state().position_secs;
    loop {
        pump(Duration::from_millis(150));
        let now = engine.state().position_secs;
        if (now - last).abs() < 0.005 || Instant::now() >= deadline {
            return now;
        }
        last = now;
    }
}

/// The mean of three channels over a whole frame, given a pixel's channel
/// offsets — which is how one function reads both the still's RGBA and the
/// playing frame's BGRA without either arm learning about the other's order.
fn mean_rgb(pixels: &[u8], red: usize, green: usize, blue: usize) -> (i32, i32, i32) {
    let count = (pixels.len() / 4).max(1) as i64;
    let (mut r, mut g, mut b) = (0_i64, 0_i64, 0_i64);
    for pixel in pixels.chunks_exact(4) {
        r += i64::from(pixel[red]);
        g += i64::from(pixel[green]);
        b += i64::from(pixel[blue]);
    }
    ((r / count) as i32, (g / count) as i32, (b / count) as i32)
}

/// RED — **a video plays: pictures keep arriving and the clock keeps
/// running**, on both recordings and on whichever engine this machine has.
///
/// The two halves are both needed and neither is the other: an engine that
/// produced one picture and stopped would pass a clock assertion, and one that
/// ran a clock over a frozen picture would pass a frame-count assertion. The
/// picture is also checked for ink, for `video::SEEK_FRACTION`'s reason — these
/// fixtures open on black, so a build that served the decoder's first buffer for
/// ever would satisfy every other line here.
///
/// The cost line is printed and nothing is concluded from it; it is §7.42 ③'s
/// measurement made again on the other platform, and §13.35 ⑥ is where the two
/// tables are read side by side.
///
/// MUTATION: never advance `generation`, and the frame count stays at zero;
/// publish a fixed `position_secs`, and the clock assertion names it.
fn a_video_plays_and_its_clock_runs_on_either_machine() {
    for (name, length) in [("folio-video-test.mp4", 5.0), ("folio-video-test.mov", 3.0)] {
        let Some(mut engine) = ready_engine(name) else {
            continue;
        };
        let opened = engine.state();
        assert_eq!(opened.natural_size, Some(NATIVE), "{name}: {opened:?}");
        let declared = opened.duration_secs.expect("a declared length");
        assert!(
            (declared - length).abs() < 0.05,
            "{name} says it is {declared} s and it is {length} s"
        );
        assert!(opened.has_video, "{name} has a picture: {opened:?}");

        let started = Instant::now();
        engine.play();
        let mut frames = Vec::new();
        let deadline = Instant::now() + PATIENCE;
        let mut advanced = false;
        while Instant::now() < deadline && (frames.len() < 3 || !advanced) {
            if let Some(frame) = engine.frame() {
                frames.push(frame);
            }
            if engine.state().position_secs > 0.05 {
                advanced = true;
            }
            pump(Duration::from_millis(4));
        }
        let elapsed = started.elapsed();
        let cost = engine.frame_cost();
        println!(
            "  VIDEO_PLAYBACK file={name} frames={} in={elapsed:?} transfer={:?} readback={:?} \
             copy={:?} total={:?} state={:?}",
            cost.frames,
            cost.transfer,
            cost.readback,
            cost.copy,
            cost.total(),
            engine.state()
        );
        assert!(
            frames.len() >= 3,
            "{name}: three pictures in {PATIENCE:?}, got {} ({:?})",
            frames.len(),
            engine.state()
        );
        assert!(advanced, "{name}: the clock ran: {:?}", engine.state());
        for pair in frames.windows(2) {
            assert!(
                pair[1].generation > pair[0].generation,
                "{name}: a generation never goes backwards"
            );
        }
        let last = frames.last().expect("three pictures");
        assert_eq!((last.width, last.height), NATIVE, "{name}");
        assert_eq!(
            last.bgra.len(),
            (NATIVE.0 * NATIVE.1 * 4) as usize,
            "{name}: no padding survives the copy"
        );
        let lit = frames.iter().any(|frame| {
            frame
                .bgra
                .chunks_exact(4)
                .any(|pixel| pixel[..3] != [0, 0, 0])
        });
        assert!(lit, "{name}: the pictures are all the opening black frame");
        engine.shutdown();
    }
}

/// RED — **pressing play does not change the colour of the picture.**
///
/// §7.42 ⑤ made the still and the first played frame share a *rectangle*,
/// because a reader who presses ▶ and sees the picture change size is watching
/// this window contradict itself about what "fill the pane" means. The same
/// sentence is true of colour and had never been asserted, and on this platform
/// it is a real question rather than a formality: the still is drawn through a
/// bitmap context that **names sRGB** and is colour-matched into it, and the
/// playing frame comes out of the player's own `32BGRA` output by a road of its
/// own. Both means are printed, and §13.35 ⑦ is where the four numbers are read.
///
/// The fixtures are one solid colour after their first fifth of a second, so
/// "the same colour" is a whole-frame mean on both sides rather than a pixel
/// hunt.
///
/// MUTATION: hand the frame over as RGBA and the two means are ~180 apart on
/// both files.
fn the_still_and_the_playing_picture_are_the_same_colour() {
    for name in ["folio-video-test.mp4", "folio-video-test.mov"] {
        let still = first_frame(&fixture(name), NATIVE.0, NATIVE.1);
        let Some(mut engine) = ready_engine(name) else {
            assert!(still.is_none(), "{name}: a still with no engine to play it");
            return;
        };
        let still = still.expect("a machine that plays a file can also still it");
        // RGBA out of the picture channel, BGRA out of the player.
        let stilled = mean_rgb(&still.rgba, 0, 1, 2);

        // **Past the black opening, and it takes two steps rather than one.**
        // Every one of these files opens on a fifth of a second of black, and
        // `frame()` hands over the picture that is *standing* — which a moment
        // after a play is still that one, whatever the clock says. So: run the
        // clock well past the opening, throw away everything taken before it got
        // there, and then wait for the next picture, which is from where the
        // clock now is. Measured on Windows 2026-09-12, the one-step version
        // compared the still against a black frame and said the red channel had
        // jumped 224.
        engine.play();
        let running = settle(&engine, |state| state.position_secs > 0.6);
        assert!(
            running.position_secs > 0.6,
            "{name}: the clock ran past the black opening: {running:?}"
        );
        while engine.frame().is_some() {}
        let deadline = Instant::now() + PATIENCE;
        let mut played = None;
        while Instant::now() < deadline && played.is_none() {
            played = engine.frame();
            pump(Duration::from_millis(4));
        }
        let played = played.expect("a picture from past the black opening");
        let playing = mean_rgb(&played.bgra, 2, 1, 0);
        println!("  VIDEO_PLAYBACK colour file={name} still={stilled:?} playing={playing:?}");
        for (channel, (a, b)) in [
            ("red", (stilled.0, playing.0)),
            ("green", (stilled.1, playing.1)),
            ("blue", (stilled.2, playing.2)),
        ] {
            assert!(
                (a - b).abs() <= STILL_TO_PLAYING,
                "{name}: the {channel} channel jumps from {a} to {b} when play is pressed"
            );
        }
        engine.shutdown();
    }
}

/// RED — **a seek lands where it was asked to, and the next picture is from
/// there.**
///
/// A tenth of a second of tolerance, which is half a frame of these
/// five-frame-a-second fixtures: what is being pinned is that the playhead moved
/// to 1.5 s rather than to a key frame at zero, and a build that seeked to the
/// nearest key frame on a file whose only key frames were at zero would land at
/// zero.
///
/// MUTATION: drop the seek command on the floor and the position stays at zero.
fn a_seek_moves_the_playhead_and_the_next_picture_comes_from_there() {
    let Some(mut engine) = ready_engine("folio-video-test.mp4") else {
        return;
    };
    // Drained first, so the picture counted below is one the seek produced and
    // not one the load did.
    while engine.frame().is_some() {}
    engine.seek(1.5);
    let state = settle(&engine, |state| state.position_secs >= 1.5);
    assert!(
        state.position_secs >= 1.5 - 0.1,
        "the seek landed at {:.3}: {state:?}",
        state.position_secs
    );
    let deadline = Instant::now() + PATIENCE;
    let mut arrived = None;
    while Instant::now() < deadline && arrived.is_none() {
        arrived = engine.frame();
        pump(Duration::from_millis(4));
    }
    let frame = arrived.expect("a picture from where the head now is");
    assert_eq!((frame.width, frame.height), NATIVE);
    println!(
        "  VIDEO_PLAYBACK seek asked=1.500 landed={:.3} generation={}",
        engine.state().position_secs,
        frame.generation
    );
    engine.shutdown();
}

/// RED — **pause stops the clock and stops the pictures; play starts both
/// again.**
///
/// The two assertions a paused player is: nothing new arrives, and the position
/// is where it was. A build that paused the audio and left the video pulling
/// would pass the second and fail the first.
///
/// MUTATION: make `Command::Pause` a no-op and the position moves; keep pulling
/// frames while paused and the frame assertion names it.
fn a_pause_stops_the_clock_and_the_pictures_and_a_play_starts_them_again() {
    let Some(mut engine) = ready_engine("folio-video-test.mp4") else {
        return;
    };
    engine.play();
    let playing = settle(&engine, |state| state.playing && state.position_secs > 0.1);
    assert!(playing.playing, "the video started: {playing:?}");

    engine.pause();
    let paused = settle(&engine, |state| !state.playing);
    assert!(!paused.playing, "the video paused: {paused:?}");
    // The clock is given time to come to rest — see [`at_rest`] — and every
    // picture that was in flight when `pause` arrived is drained here rather
    // than counted below.
    let at = at_rest(&engine);
    while engine.frame().is_some() {}
    println!("  VIDEO_PLAYBACK pause rest={at:.3}");
    pump(Duration::from_millis(500));
    let after = engine.state();
    assert!(
        (after.position_secs - at).abs() < 0.01,
        "a paused clock that had come to rest at {at:.3} ran on to {:.3}: {after:?}",
        after.position_secs
    );
    assert!(
        engine.frame().is_none(),
        "a paused video drew a new picture"
    );
    assert!(
        engine.standing_frame().is_some(),
        "a paused video kept the picture it was showing"
    );

    engine.play();
    let again = settle(&engine, |state| state.position_secs > at + 0.1);
    assert!(
        again.position_secs > at,
        "play started the clock again: {again:?}"
    );
    engine.shutdown();
}

/// RED — **a video that reaches its end says so, stops, and keeps its last
/// picture.**
///
/// §M4 acceptance ③'s "ends without a stuck frame" from the engine's side: the
/// bar is told the video ended, the clock has stopped, and the picture standing
/// on the glass is the one the file finished on rather than black or nothing.
/// The three-second `.mov` is used because it is the shortest recording that is
/// long enough to see a clock run.
///
/// It also pins the **one** in "exactly once": `ended` is sticky while the head
/// is at the end, and a press of ▶ on an ended video starts it again rather than
/// doing nothing, which is `HTMLMediaElement.play`'s own rule and therefore both
/// arms'.
///
/// MUTATION: clear the standing frame when the clock stops and the last
/// assertion names it; leave `ended` false and the first one does.
fn a_video_that_ends_says_so_and_keeps_its_last_picture() {
    let Some(mut engine) = ready_engine("folio-video-test.mov") else {
        return;
    };
    engine.play();
    let ended = settle(&engine, |state| state.ended);
    assert!(ended.ended, "the video ended: {ended:?}");
    assert!(!ended.playing, "an ended video is not playing: {ended:?}");
    let standing = engine
        .standing_frame()
        .expect("an ended video keeps its last picture");
    assert_eq!((standing.width, standing.height), NATIVE);
    let lit = standing
        .bgra
        .chunks_exact(4)
        .any(|pixel| pixel[..3] != [0, 0, 0]);
    assert!(lit, "the last picture is not a black rectangle");
    println!(
        "  VIDEO_PLAYBACK end position={:.3} duration={:?} generation={}",
        ended.position_secs, ended.duration_secs, standing.generation
    );

    // It stays ended: nothing un-ends a video that nobody has touched.
    pump(Duration::from_millis(200));
    assert!(engine.state().ended, "an ended video stayed ended");

    // And ▶ starts it again rather than doing nothing.
    engine.play();
    let restarted = settle(&engine, |state| !state.ended && state.playing);
    assert!(
        !restarted.ended && restarted.playing,
        "play on an ended video started it again: {restarted:?}"
    );
    engine.shutdown();
}

/// RED — **the sound is the engine's own, and the two knobs that shape it reach
/// it.**
///
/// What this case can hold and what it cannot is written in the module note: it
/// asserts that the file has an audio track and the engine says so, that
/// `set_volume` and `set_muted` are read back off the engine rather than off a
/// copy this crate keeps, and that a muted video still plays. Whether a speaker
/// made a noise is the owner's ear, and §M4 acceptance ③ is where that is
/// recorded.
///
/// The engine is left **muted** at the end of every path through this case.
///
/// MUTATION: drop `Command::Volume` and the volume stays where it was; report
/// `has_audio` off the video track and the silent fixtures start claiming sound.
fn a_recording_with_a_soundtrack_answers_for_its_own_audio() {
    let Some(mut engine) = ready_engine("folio-video-sound-test.mp4") else {
        return;
    };
    let opened = engine.state();
    assert!(
        opened.has_audio,
        "the one fixture with a soundtrack has one: {opened:?}"
    );
    assert!(opened.has_video, "and a picture too: {opened:?}");
    assert!(opened.muted, "every engine in this file starts muted");

    engine.set_volume(0.0);
    let quiet = settle(&engine, |state| state.volume < 0.01);
    assert!(quiet.volume < 0.01, "volume 0.0 was taken: {quiet:?}");
    engine.set_volume(1.0);
    let loud = settle(&engine, |state| state.volume > 0.99);
    assert!(loud.volume > 0.99, "volume 1.0 was taken: {loud:?}");
    assert!(loud.muted, "a volume change did not un-mute anything");

    // A muted video is still a video: the clock runs and the pictures arrive.
    engine.play();
    let playing = settle(&engine, |state| state.position_secs > 0.1);
    assert!(playing.playing, "a muted video plays: {playing:?}");
    assert!(playing.muted, "and it is still muted: {playing:?}");
    engine.pause();
    engine.shutdown();

    // The other recordings are silent, and the engine says *that* too — the half
    // of this claim a build reporting `true` for everything would fail.
    for silent in ["folio-video-test.mp4", "folio-video-test.mov"] {
        let Some(mut engine) = ready_engine(silent) else {
            return;
        };
        assert!(
            !engine.state().has_audio,
            "{silent} has no soundtrack: {:?}",
            engine.state()
        );
        engine.shutdown();
    }
}

/// RED — **a rate is a rate and not a play button.**
///
/// `AVPlayer.rate` is the pause control as well as the speed and
/// `IMFMediaEngine`'s is not, so this is the one number the two arms compute
/// differently and must answer the same. A paused player asked for one and a
/// half reads back one and a half, and is still paused.
///
/// MUTATION: report `AVPlayer.rate` as `EngineState::rate` and the first
/// assertion reads zero on the Mac; start the clock on a rate change and the
/// second names it.
fn a_rate_set_on_a_paused_video_is_a_rate_and_not_a_play() {
    let Some(mut engine) = ready_engine("folio-video-test.mp4") else {
        return;
    };
    engine.set_rate(1.5);
    let state = settle(&engine, |state| (state.rate - 1.5).abs() < 0.01);
    assert!(
        (state.rate - 1.5).abs() < 0.01,
        "the rate was taken: {state:?}"
    );
    assert!(!state.playing, "a rate is not a play: {state:?}");
    engine.shutdown();
}

/// RED — **nothing that is not a video plays, and nothing panics saying so.**
///
/// Two refusals the product really meets: a path with nothing behind it, and a
/// file whose name says `.mp4` and whose bytes say otherwise. Either may be
/// refused at `open` or a moment later in `EngineState::error` — loading is
/// asynchronous on both platforms — and what may not happen is a picture.
///
/// MUTATION: let a track-less asset through and the error assertion names it.
fn nothing_that_is_not_a_video_plays() {
    let dir = std::env::temp_dir().join(format!(
        "folio-video-playback-refusals-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let text = dir.join("renamed.mp4");
    std::fs::write(&text, b"this is not a video at all, whatever it is called")
        .expect("a text file");
    for path in [dir.join("no-such-file.mp4"), text] {
        match Engine::open(&path) {
            Ok(mut engine) => {
                engine.set_muted(true);
                let deadline = Instant::now() + PATIENCE;
                while Instant::now() < deadline && engine.state().error.is_none() {
                    pump(Duration::from_millis(10));
                }
                let state = engine.state();
                assert!(
                    state.error.is_some(),
                    "{} loaded: {state:?}",
                    path.display()
                );
                assert!(
                    engine.frame().is_none(),
                    "{} drew a picture",
                    path.display()
                );
                // Sticky: an engine that has errored does not un-error.
                pump(Duration::from_millis(100));
                assert_eq!(engine.state().error, state.error, "{}", path.display());
                engine.shutdown();
            }
            // Only a machine with no decoder at all refuses here: both real arms
            // open a thread and put the refusal in the state, which is the
            // branch above.
            Err(error) => assert_eq!(error, EngineError::Unsupported, "{}", path.display()),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// RED — **an engine that is dropped while it is playing is shut down.**
///
/// The ledger's whole promise, asserted through the door that forgets: no
/// `shutdown` call anywhere in the scope, a video mid-play when the scope ends,
/// and `engines_outstanding` back where it started. A build whose `Drop` had
/// gone would leave a decoder, a thread and an audio output behind for every
/// pane that was closed by anything other than its own button.
///
/// MUTATION: remove `impl Drop for Engine` and the count never comes back.
fn an_engine_dropped_while_it_plays_leaves_nothing_behind() {
    let before = engines_outstanding();
    let started_before = engines_started();
    {
        let Some(engine) = ready_engine("folio-video-test.mp4") else {
            return;
        };
        engine.play();
        let playing = settle(&engine, |state| state.position_secs > 0.1);
        assert!(playing.playing, "the video was playing when it was dropped");
        assert!(
            engines_started() > started_before,
            "the ledger counted the engine that was opened"
        );
        // No `shutdown`. The scope ends here and `Drop` is the whole case.
    }
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline && engines_outstanding() > before {
        pump(Duration::from_millis(10));
    }
    assert_eq!(
        engines_outstanding(),
        before,
        "a dropped engine stayed on the ledger ({} started, {} shut down)",
        engines_started(),
        engines_shut_down()
    );
}

/// PIN — **a machine with no decoder refuses, and says so the one way every
/// caller already reads.**
///
/// The third platform's whole surface. It is a case rather than a comment
/// because the shape of a refusal is what `bt-app` prints a line from, and a
/// build whose refusal became a panic would take the window with it.
fn a_machine_with_no_decoder_refuses_every_video() {
    if HAS_ENGINE {
        return;
    }
    for name in ["folio-video-test.mp4", "folio-video-test.mov"] {
        assert_eq!(
            Engine::open(&fixture(name)).err(),
            Some(EngineError::Unsupported),
            "{name}"
        );
    }
}

/// **What a `harness = false` binary owes the two gates that ask libtest
/// questions of every test binary in the workspace.**
///
/// `scripts/ci/check-ignored-tests.ps1` runs `cargo test --workspace -- --ignored
/// --list` and reads every binary's answer; a target with its own `main` is
/// handed those arguments and, unless it says otherwise, runs its whole suite as
/// if nobody had asked anything. The other `harness = false` targets here never
/// noticed because each exits at once without its environment variable — this
/// one has none, so it says it here: **asked to list, it lists nothing; asked for
/// the ignored ones, it runs none**, because it has no ignored cases and the
/// honest answer to both questions is an empty one.
fn asked_a_question_rather_than_told_to_run() -> bool {
    std::env::args().any(|argument| argument == "--list" || argument == "--ignored")
}

fn main() {
    if asked_a_question_rather_than_told_to_run() {
        return;
    }
    for (name, case) in [
        (
            "a_video_plays_and_its_clock_runs_on_either_machine",
            a_video_plays_and_its_clock_runs_on_either_machine as fn(),
        ),
        (
            "the_still_and_the_playing_picture_are_the_same_colour",
            the_still_and_the_playing_picture_are_the_same_colour,
        ),
        (
            "a_seek_moves_the_playhead_and_the_next_picture_comes_from_there",
            a_seek_moves_the_playhead_and_the_next_picture_comes_from_there,
        ),
        (
            "a_pause_stops_the_clock_and_the_pictures_and_a_play_starts_them_again",
            a_pause_stops_the_clock_and_the_pictures_and_a_play_starts_them_again,
        ),
        (
            "a_video_that_ends_says_so_and_keeps_its_last_picture",
            a_video_that_ends_says_so_and_keeps_its_last_picture,
        ),
        (
            "a_recording_with_a_soundtrack_answers_for_its_own_audio",
            a_recording_with_a_soundtrack_answers_for_its_own_audio,
        ),
        (
            "a_rate_set_on_a_paused_video_is_a_rate_and_not_a_play",
            a_rate_set_on_a_paused_video_is_a_rate_and_not_a_play,
        ),
        (
            "nothing_that_is_not_a_video_plays",
            nothing_that_is_not_a_video_plays,
        ),
        (
            "an_engine_dropped_while_it_plays_leaves_nothing_behind",
            an_engine_dropped_while_it_plays_leaves_nothing_behind,
        ),
        (
            "a_machine_with_no_decoder_refuses_every_video",
            a_machine_with_no_decoder_refuses_every_video,
        ),
    ] {
        case();
        println!("{name}: ok");
    }
    println!("video_playback: 10 cases, all ok");
}
