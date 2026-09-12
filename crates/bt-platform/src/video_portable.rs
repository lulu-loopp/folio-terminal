//! **Video, on a platform whose decoder has not been written yet** (M4-4 for
//! the first frame, M4-5 for playback).
//!
//! `bt-app` names eleven things from `video` and `video::engine`, and every one
//! of them is here. What is behind them is nothing: `first_frame` answers
//! `None`, which the hover card already reads as *no picture for this file*,
//! and `Engine::open` answers `EngineError::Unsupported`, which the video pane
//! already reads as *this machine cannot play this*, prints under a black
//! rectangle, and goes on.
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
//! implementation and a refusal. Until then the duplication is four structs
//! with no behaviour, and the cost of getting one of them wrong is a compile
//! error in `bt-app` rather than a silent divergence.

use std::path::Path;
use std::time::Duration;

/// How far into a video the first frame is taken from — the same tenth the
/// Windows arm seeks to, because the reason is the content and not the API: the
/// first frame of a great many videos is black.
pub const SEEK_FRACTION: f64 = 0.10;

/// How long a first frame may take before the card gives up on it.
pub const FIRST_FRAME_BUDGET: Duration = Duration::from_secs(3);

/// One decoded picture, fitted for a card. See the module note on why this is
/// written twice.
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

/// **The first frame of a video, for the hover card and the preview pane**
/// (M4-4: `AVAssetImageGenerator`).
///
/// `None` is the refusal, and it is the same `None` the Windows arm answers for
/// a container it has no decoder for: the card shows the file's name and no
/// picture. Nothing above this treats it as an error, which is why this is the
/// one shape a refusal can take here.
#[must_use]
pub fn first_frame(path: &Path, fit_width: u32, fit_height: u32) -> Option<VideoFrame> {
    let _ = (path, fit_width, fit_height);
    None
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
        /// Whether the source is silent — no audio stream, or muted.
        pub muted: bool,
        /// What went wrong, if anything has.
        pub error: Option<EngineError>,
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
