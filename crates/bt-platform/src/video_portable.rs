//! **Video off Windows: a real first frame on a Mac, and nothing anywhere that
//! can play one yet** (M4-4 landed here; M4-5 is still ahead).
//!
//! `bt-app` names eleven things from `video` and `video::engine`, and every one
//! of them is here. Three of them now do something: [`first_frame`],
//! [`decode_first_frame`] and [`decode_first_frame_measured`] reach
//! `AVAssetImageGenerator` on macOS through `src/macos_video.rs`, and answer
//! `None` on a third platform exactly as they did before. The rest is still
//! nothing: `Engine::open` answers `EngineError::Unsupported`, which the video
//! pane reads as *this machine cannot play this*, prints under a black
//! rectangle, and goes on.
//!
//! # Two platforms, one file, and where the cut is
//!
//! What is in *this* file is everything that is not a decoder — the frame's
//! shape, the two timing constants, the cost breakdown, the fit, the giving-up,
//! and the engine's refusal — because none of that is AVFoundation and all of it
//! is the same sentence on a Mac and on a machine with neither backend. What is
//! in `macos_video.rs` is the AVFoundation conversation and nothing else. The
//! split follows `handoff.rs`, where `macos_handoff` sits beside
//! `portable_handoff` and `lib.rs` picks one: a body per platform, one set of
//! names, and a `cfg` on the arms rather than on the call sites.
//!
//! # The one duplication in this ticket, and why it is here
//!
//! Four small data types — [`VideoFrame`], [`EngineError`], [`EngineState`] and
//! [`Frame`] — are written twice: once in `video/mod.rs` and `video/engine.rs`
//! against Media Foundation, and once here. Everywhere else M1-1 moved a shared
//! definition into the ungated half of its file rather than copying it
//! (`webview.rs` is the example), and that is the better answer.
//!
//! It is not the answer here because those two files are Media Foundation from
//! their first line to their last — an `IMFMediaEngine` on a worker thread, a
//! D3D11 texture, a staging read-back — and the types are interleaved with it
//! rather than gathered. **M4-4 and M4-5 replace both files with AVFoundation**
//! (`AVAssetImageGenerator`, then `AVPlayer` with audio, seeking and colour
//! conversion), and the right moment to have one definition of a frame is when
//! there are two real implementations to share it, not when there is one
//! implementation and a refusal.
//!
//! **M4-4 is half of that moment and does not take it**, deliberately. There
//! are now two real implementations of the *first frame* and still only one of
//! the engine, so gathering [`VideoFrame`] into a shared file today would move
//! it out from beside `Frame`, `EngineError` and `EngineState`, which would
//! still be written twice — one definition shared and three copied is a worse
//! shape to read than four copied. What holds the two [`VideoFrame`]s to each
//! other in the meantime is not a convention: `tests/video_first_frame.rs` runs
//! the same assertions against whichever arm the machine compiled, and
//! `lib.rs`'s `macos_video_signature_tests` compares the two arms' text on the
//! Windows machine, where only one of them can be built.

use std::time::Duration;

/// How far into a video the first frame is taken from — the same tenth the
/// Windows arm seeks to, because the reason is the content and not the API: the
/// first frame of a great many videos is black.
pub const SEEK_FRACTION: f64 = 0.10;

/// How long a first frame may take before the card gives up on it.
pub const FIRST_FRAME_BUDGET: Duration = Duration::from_secs(3);

/// One decoded picture, fitted for a card. See the module note on why this is
/// written twice.
///
/// **`rgba` is straight (non-premultiplied) RGBA8, row-major, packed at
/// `width * 4` bytes a row, and fully opaque** — the same sentence the Windows
/// arm's own `VideoFrame` opens with, and it
/// is the whole contract between a decoder and the picture channel this frame
/// joins. `width`/`height` are the raster's; `native_width`/`native_height` are
/// the video's own, which is what the fact line prints and would be quietly
/// wrong if it were read off the pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// How long the video is, in milliseconds, or `None` when the container
    /// does not declare a duration.
    pub duration_ms: Option<u64>,
    pub native_width: u32,
    pub native_height: u32,
}

/// **Where one question's time went**, segment by segment — the twin of the
/// Windows arm's `FirstFrameCost`, with the same six fields for the same six
/// reasons.
///
/// The fields are named after Media Foundation's shape because that is the arm
/// that measured the problem first and because a caller that matched on one name
/// here and another there would be two callers. What each one *means* on
/// AVFoundation is written on it, and one of the six is always zero: there is no
/// media platform to start on a Mac, which is the same fact `prewarm` is a no-op
/// for.
///
/// A segment a refusal never reached stays zero. Nothing in this module reads
/// these back or decides anything by them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FirstFrameCost {
    /// Starting the media platform. **Always zero here**: AVFoundation has no
    /// `MFStartup` and no apartment to join, so the cold question costs what the
    /// warm one costs.
    pub session: Duration,
    /// Building the `AVURLAsset` and its `AVAssetImageGenerator`.
    pub open: Duration,
    /// Settling what is being asked for: the video track, its natural size and
    /// its preferred transform, and the generator's cap and tolerances.
    pub output_type: Duration,
    /// Reading the declared duration and turning it into the time
    /// [`SEEK_FRACTION`] names.
    pub seek: Duration,
    /// `copyCGImageAtTime:actualTime:error:` — the decode itself.
    pub read_sample: Duration,
    /// Drawing the `CGImage` into a bitmap context and taking the rows out of
    /// it.
    pub copy: Duration,
}

impl FirstFrameCost {
    /// The six spans added up — the length of the whole question.
    #[must_use]
    pub fn total(self) -> Duration {
        self.session + self.open + self.output_type + self.seek + self.read_sample + self.copy
    }
}

/// **The AVFoundation arm** — everything M4-4 wrote, and nothing that is not
/// AVFoundation. See its own header.
#[cfg(target_os = "macos")]
#[path = "macos_video.rs"]
mod macos_video;

/// **The three first-frame doors, on a Mac.**
#[cfg(target_os = "macos")]
pub use macos_video::{decode_first_frame, decode_first_frame_measured, first_frame};

/// **The three first-frame doors, on a platform with neither Media Foundation
/// nor AVFoundation** — still the one silence.
#[cfg(not(target_os = "macos"))]
pub use no_decoder::{decode_first_frame, decode_first_frame_measured, first_frame};

/// **A third platform, where there is no decoder to ask** — Linux today, and
/// the 0.5 remote server's host tomorrow.
///
/// `None` is the refusal, and it is the same `None` the Windows arm answers for
/// a container it has no decoder for: the card shows the file's name and no
/// picture. Nothing above this treats it as an error, which is why this is the
/// one shape a refusal can take here.
#[cfg(not(target_os = "macos"))]
mod no_decoder {
    use std::path::Path;

    use super::{FirstFrameCost, VideoFrame};

    /// **The first frame of a video, for the hover card and the preview pane.**
    #[must_use]
    pub fn first_frame(path: &Path, fit_width: u32, fit_height: u32) -> Option<VideoFrame> {
        let _ = (path, fit_width, fit_height);
        None
    }

    /// The same answer with no clock over it.
    #[must_use]
    pub fn decode_first_frame(path: &Path, fit_width: u32, fit_height: u32) -> Option<VideoFrame> {
        decode_first_frame_measured(path, fit_width, fit_height).0
    }

    /// The same answer, and where its milliseconds went — which here is
    /// nowhere, because nothing was asked of anything.
    #[must_use]
    pub fn decode_first_frame_measured(
        path: &Path,
        fit_width: u32,
        fit_height: u32,
    ) -> (Option<VideoFrame>, FirstFrameCost) {
        (
            first_frame(path, fit_width, fit_height),
            FirstFrameCost::default(),
        )
    }
}

/// `size` fitted inside `fit` with its proportions kept, and **never enlarged**.
///
/// The Windows arm's `contain`, word for word, and it is written twice for the
/// reason [`VideoFrame`] is: the two files share no code today. `contain` rather
/// than `cover`, which is the same bargain every other picture in this window is
/// fitted by — a wide frame and a tall one are both themselves, and the host
/// centres what is left over. The clamp against `size` is what keeps a small clip
/// from being asked for at a size whose pixels do not exist.
#[cfg(target_os = "macos")]
fn contain(size: (u32, u32), fit: (u32, u32)) -> (u32, u32) {
    let scale = (f64::from(fit.0) / f64::from(size.0)).min(f64::from(fit.1) / f64::from(size.1));
    if scale >= 1.0 {
        return size;
    }
    (
        ((f64::from(size.0) * scale).round() as u32).clamp(1, size.0),
        ((f64::from(size.1) * scale).round() as u32).clamp(1, size.1),
    )
}

/// **Run `work` on a thread of its own and stop waiting for it after `budget`.**
///
/// The Windows arm's `within_budget`, and the same two endings a caller cannot
/// tell apart and does not need to: the work answered `None`, or it had not
/// answered at all when the budget ran out.
///
/// The thread that overran is **not** cancelled. `AVAssetImageGenerator` does
/// have a `cancelAllCGImageGeneration`, and it is not used here, because it
/// cancels the *asynchronous* requests a generator is holding and there is no
/// supported way to interrupt `copyCGImageAtTime:` from outside — the same
/// sentence Media Foundation's `ReadSample` gets. The thread is left to finish
/// into a receiver nobody is holding, which drops its answer exactly where the
/// caller would have dropped it, and then to unwind. That is why the work it is
/// given holds no lock and writes to nothing but its own channel.
#[cfg(target_os = "macos")]
fn within_budget<T: Send + 'static>(
    budget: Duration,
    work: impl FnOnce() -> Option<T> + Send + 'static,
) -> Option<T> {
    let (answer, wait) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("folio-video-frame".to_owned())
        .spawn(move || {
            let _ = answer.send(work());
        })
        .ok()?;
    wait.recv_timeout(budget).ok().flatten()
}

/// Warm the media platform up before the first card asks for a frame.
///
/// A no-op, and one of the plan's §4.4 class-N items rather than deferred work:
/// `MFStartup` is a Windows requirement that AVFoundation does not have, so
/// there is nothing to warm and nothing downstream notices.
pub fn prewarm() {}

/// Shut the media platform down on the way out.
///
/// The other half of [`prewarm`], and a no-op for the same reason.
pub fn shutdown_media_session() {}

/// **Playback** (M4-5: `AVPlayer`, with the audio the spike never costed).
pub mod engine {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    /// How often a playing pane asks for a new frame.
    pub const FRAME_POLL_INTERVAL: Duration = Duration::from_millis(4);
    /// How often a paused one does.
    pub const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(50);
    /// How long opening a source may take.
    pub const OPEN_BUDGET: Duration = Duration::from_secs(5);
    /// How long shutting one down may take.
    pub const SHUTDOWN_BUDGET: Duration = Duration::from_secs(2);

    /// **Why an engine could not be made, or could not play what it was
    /// given.** See the module note on why this is written twice.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum EngineError {
        /// The load was stopped — `MediaError.MEDIA_ERR_ABORTED`.
        Aborted,
        /// The bytes stopped arriving — `MediaError.MEDIA_ERR_NETWORK`.
        Network,
        /// There is a decoder and it could not decode this — `MEDIA_ERR_DECODE`.
        Decode,
        /// There is no decoder, or no source, for this container or codec.
        Unsupported,
        /// Something else went wrong, and it says what.
        Other(&'static str),
    }

    /// What the engine is doing right now. See the module note.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct EngineState {
        /// How long the video is, or `None` before the metadata has arrived.
        pub duration_secs: Option<f64>,
        /// Where the playhead is, in seconds from the start.
        pub position_secs: f64,
        /// Whether the engine is running the clock.
        pub playing: bool,
        /// The video's own pixel dimensions, or `None` before the metadata has
        /// arrived and for a source with no video stream at all.
        pub natural_size: Option<(u32, u32)>,
        /// Whether the playhead has reached the end.
        pub ended: bool,
        /// The one thing that went wrong, if one did. Sticky.
        pub error: Option<EngineError>,
        pub muted: bool,
        /// `0.0`–`1.0`, the same scale `HTMLMediaElement.volume` uses.
        pub volume: f64,
        /// `1.0` is normal speed.
        pub rate: f64,
        /// Whether the metadata has arrived.
        pub ready: bool,
        /// Whether this source has a picture at all.
        pub has_video: bool,
        pub has_audio: bool,
    }

    /// One frame on its way to the renderer. See the module note.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Frame {
        pub bgra: Arc<[u8]>,
        pub width: u32,
        pub height: u32,
        /// Counts from one and never repeats for the life of an engine.
        pub generation: u64,
    }

    /// **A video being played, before there is anything to play it with.**
    ///
    /// Refuses at `open`, which is where the video pane already has somewhere
    /// to put a reason: a line under a black rectangle naming the error, which
    /// is a different product from a pane that shows nothing.
    pub struct Engine {
        /// Never constructed: [`Engine::open`] refuses.
        _never: std::convert::Infallible,
    }

    impl Engine {
        /// Open a source. Refused; M4-5.
        pub fn open(path: &Path) -> Result<Self, EngineError> {
            let _ = path;
            Err(EngineError::Unsupported)
        }

        /// Unreachable: there is no value of this type.
        #[must_use]
        pub fn source(&self) -> &Path {
            match self._never {}
        }

        /// Unreachable.
        #[must_use]
        pub fn state(&self) -> EngineState {
            match self._never {}
        }

        /// Unreachable.
        pub fn frame(&mut self) -> Option<Frame> {
            match self._never {}
        }

        /// Unreachable.
        pub fn play(&self) {
            match self._never {}
        }

        /// Unreachable.
        pub fn pause(&self) {
            match self._never {}
        }

        /// Unreachable.
        pub fn seek(&self, secs: f64) {
            let _ = secs;
            match self._never {}
        }

        /// Unreachable.
        pub fn set_rate(&self, rate: f64) {
            let _ = rate;
            match self._never {}
        }

        /// Unreachable.
        pub fn set_muted(&self, muted: bool) {
            let _ = muted;
            match self._never {}
        }

        /// Unreachable.
        pub fn set_volume(&self, volume: f64) {
            let _ = volume;
            match self._never {}
        }

        /// Unreachable.
        pub fn shutdown(&mut self) {
            match self._never {}
        }

        /// The path this engine was opened on, as an owned value — unreachable.
        #[must_use]
        pub fn source_path(&self) -> PathBuf {
            match self._never {}
        }
    }

    /// **The leak ledger**, and it is real on every platform.
    ///
    /// Three atomics and nothing else — the Windows arm's own counters are the
    /// same three — so `main.rs`'s tests that a closed pane leaves no engine
    /// behind mean the same thing here: zero started, zero shut down, zero
    /// outstanding is a true reading of a platform that opens none.
    static STARTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static SHUT_DOWN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// How many engines this process has started.
    #[must_use]
    pub fn engines_started() -> u64 {
        STARTED.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many it has shut down.
    #[must_use]
    pub fn engines_shut_down() -> u64 {
        SHUT_DOWN.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many are still open. Zero, always, because none is ever started.
    #[must_use]
    pub fn engines_outstanding() -> u64 {
        engines_started().saturating_sub(engines_shut_down())
    }
}
