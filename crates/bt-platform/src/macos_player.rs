//! **A video that is playing on a Mac: `AVPlayer` for the sound and the clock,
//! `AVPlayerItemVideoOutput` for the pictures** (M4-5; `docs/DESIGN.md` §7.42,
//! §7.44 and §13.35).
//!
//! [`super::macos_video`] answers one question about a file — what does a frame
//! of it look like — and gives the platform back. This answers the other one:
//! keep answering it while a clock runs and a speaker plays, until somebody says
//! stop. It is the macOS twin of `video/engine.rs`, and the contract it keeps is
//! that file's rather than AVFoundation's: the same verbs, the same
//! [`EngineState`], the same straight BGRA8 [`Frame`], the same ledger, and the
//! same promise that a caller never learns which machine it is on.
//!
//! # `AVPlayer` is `<video>`, which is what the other arm said about `IMFMediaEngine`
//!
//! The Windows arm's whole case for leaving the browser was that Media
//! Foundation's media engine *is* the HTML5 media element in COM form, so what
//! is lost is the chrome and not the player. The same sentence is true here and
//! it is the reason this file is short: `play`, `pause`, `seekToTime:`, `rate`,
//! `muted`, `volume`, `duration`, and an item that posts a notification when it
//! has played to its end. One verb for one verb.
//!
//! **Audio is the half the port's spike never costed, and on this platform it
//! costs nothing at all.** An `AVPlayer` has an audio output of its own: the
//! sound of the file is playing the instant `play` is answered, and
//! [`Engine::set_volume`] and [`Engine::set_muted`] are the two properties that
//! shape it. Nothing in this file opens a device, mixes a buffer or names a
//! sample rate — which is precisely what "the engine owns the audio" meant in
//! §7.42's fourth trigger, and the reason this ticket is not an audio ticket.
//!
//! # The pictures come out of a second object, on purpose and on Apple's advice
//!
//! An `AVPlayer` on its own draws through an `AVPlayerLayer`, which would put a
//! second compositor inside a window that already has one — the same refusal the
//! Windows arm made of Media Foundation's *rendering mode*, for the same reason.
//! What replaces it is the same third shape: a **pull**.
//! `AVPlayerItemVideoOutput` is added to the item, asked
//! `hasNewPixelBufferForItemTime:` and, when it says yes,
//! `copyPixelBufferForItemTime:` — and *where* and *when* the picture is drawn
//! stay on this side of the call.
//!
//! The output is asked for **`kCVPixelFormatType_32BGRA`**, which is B, G, R, A
//! in memory in that order, because that is [`Frame`]'s promise and the byte
//! order the swapchain this window presents through already uses. Nothing in
//! this file reorders a channel; the only per-pixel work is a `memcpy`.
//!
//! **The stride is read back rather than assumed.** A `CVPixelBuffer`'s
//! `bytesPerRow` is padded to whatever alignment the decoder wanted and nothing
//! promises it is `width * 4`, while [`Frame::bgra`] is packed at `width * 4`
//! with no padding — a pitch is a fact about somebody's memory and not about a
//! picture. So the copy walks the rows. Whether a *particular* file's buffer is
//! padded is not observable from outside that copy and is not the point:
//! [`copy_tight`] is the whole behaviour and its three unit tests state it on a
//! padded buffer, on a tight one, and on a stride too narrow to describe the
//! picture it claims to — none of which needs a Mac or a video.
//!
//! # The thread, and the one place this ticket's brief was wrong
//!
//! The brief said there is no main-thread requirement here and that this file
//! should say so with evidence, as M4-4 did for `AVAssetImageGenerator`. **The
//! evidence says the opposite of half of it, so this file says that instead.**
//! Measured in the macOS 26.6 SDK on the venue machine, 2026-09-12:
//! `AVFoundation/AVPlayer.h` and `AVFoundation/AVPlayerItem.h` both carry
//! `NS_SWIFT_UI_ACTOR` on the `@interface` line — Apple's spelling of
//! `@MainActor` — and `objc2` translates that annotation into
//! `#[thread_kind = MainThreadOnly]`, which is why `AVPlayer::playerWithURL` and
//! every other constructor of those two classes takes a `MainThreadMarker`.
//! `AVFoundation/AVPlayerItemOutput.h` carries no such annotation, and
//! `AVPlayerItemVideoOutput` is `Send + Sync` in the same bindings.
//!
//! So the two objects this file has to make are annotated for Swift's main
//! actor, and this file makes them **on the engine's own thread anyway**, with
//! [`alloc_off_the_window_thread`] — a raw `alloc` message to the class, which
//! is the one call the marker guards. That is a decision and not an oversight,
//! and it rests on four things rather than on convenience:
//!
//! 1. **`NS_SWIFT_UI_ACTOR` is a Swift concurrency annotation, not an
//!    Objective-C threading contract.** It exists so that Swift code touching
//!    these classes is isolated to the main actor. Apple's Objective-C reference
//!    for `AVPlayer` states no thread requirement, and the *Thread Safety
//!    Summary* in the Cocoa Multithreading Programming Guide — the list
//!    `handoff.rs` and `macos_video.rs` both cite — does not name it among the
//!    classes that are the main thread's, saying of everything it does not list
//!    that "in most cases, you can use these classes from any thread as long as
//!    you use them from only one thread at a time". **One thread at a time is
//!    exactly what this module is**: the player, the item and the output are
//!    born on the engine thread, live in a struct that never leaves it, and die
//!    on it.
//! 2. **The header's own sentence is about callbacks, not about calls.** Both
//!    files say the class "serializes notifications of changes that occur
//!    dynamically during playback on a dispatch queue. By default, this queue is
//!    the main queue" — a statement about where a *notification* is delivered.
//!    This module reads no notification for a value; see the next section.
//! 3. **The pull path Apple documents is not on the main thread.**
//!    `AVPlayerItemVideoOutput`'s own reference says to call
//!    `copyPixelBufferForItemTime:` "in response to a `CADisplayLink` delegate
//!    invocation", which is a display-link thread. A player whose frames may
//!    only be taken off the main thread and whose pictures must be taken off a
//!    display link would be a contradiction Apple published.
//! 4. **The measurement.** `tests/video_playback.rs` runs every case on a thread
//!    libtest spawned, where `MainThreadMarker::new()` is `None` — §13.17
//!    measured that, and it is why the sheet cases needed a target of their own
//!    — and there is no run loop on the process's first thread while they run.
//!    A green run of that file is the statement, exactly as §13.25 ⑥ said of the
//!    first frame.
//!
//! What this file will *not* do is claim the marker away: nothing here builds a
//! main-thread marker, checked or unchecked, because that would be a lie about
//! *which thread this is* rather than a statement about what the class needs —
//! and a marker made on a worker makes every main-thread-only door in AppKit
//! reachable from it. `alloc` on a class object is the narrowest step there is:
//! one message, to one class, in one function, with the reason written on it.
//! `the_player_is_allocated_without_claiming_the_window_thread` in `lib.rs` is
//! the gate over that sentence.
//!
//! # Events wake the loop; the player answers the questions
//!
//! §7.42 ④'s rule is kept word for word. The one notification this module
//! registers for — `AVPlayerItemDidPlayToEndTimeNotification`, scoped to this
//! engine's own item — arrives on whatever thread AVFoundation posted it from,
//! and the only thing it does there is push a [`Command`] down the same channel
//! the verbs use. **Nothing touches a window, a layout or a renderer from inside
//! it**, and there is nothing here that could: the observer's whole world is a
//! `Sender`.
//!
//! The end is the one fact this module *records* rather than re-reads, and that
//! is the same exception the Windows arm makes for an error code. "The playhead
//! is at the end" cannot be spelled as a comparison: a five-second file whose
//! last picture is at 4.8 s leaves `currentTime` short of `duration` by a frame
//! nobody knows the length of, and `rate == 0` is the same reading a pause
//! gives. The item's own notification is the platform saying it, and — like
//! `HTMLMediaElement.ended`, which is what both arms are — it is cleared by a
//! seek back into the file and by a [`Command::Play`] that restarts one.
//!
//! # Lifetime
//!
//! One engine per playing video, [`Engine::shutdown`] on the pane that closes
//! and [`Drop`] for everything else including a panic, both ending in the same
//! place: the thread is told to stop, it removes the observer, removes the
//! output from the item, pauses and empties the player, and the handle joins it.
//! The observer is removed **before** the item it is scoped to, because a
//! notification centre holds its observers unretained and one left behind is a
//! pointer to a freed object the next time anything posts.
//!
//! `super::engine::engines_started` and `engines_shut_down` count the two halves
//! here for the reason they count them there: so that "no engine outlives the
//! process" is a claim a test can read rather than a promise in a comment.
//!
//! # A pool per turn, because this is a thread of our own (RA-5)
//!
//! Apple's contract for a secondary thread that touches Cocoa is that the
//! thread makes an autorelease pool before it sends its first message and
//! drains one periodically if it is long-lived; a thread that AppKit did not
//! start has no pool of its own, and what is autoreleased on it is held until
//! there is one. This file's own calls are nearly all scalars and `CMTime`s and
//! everything it *owns* is a `Retained` — so what has nowhere to go is not
//! this module's objects but whatever AVFoundation autoreleases internally on
//! the way through, and [`Machinery::pump`] goes through it every
//! [`FRAME_POLL_INTERVAL`] for as long as the preview is open.
//!
//! So there are two pools and the inner one is the point. [`run`] holds one
//! around the whole of the thread's life, which is what covers building the
//! machinery and tearing it down; `pump` opens and drains a nested one **every
//! turn**, which is what keeps a five-minute video from accumulating five
//! minutes of autoreleased objects. A single pool around the whole lifetime is
//! not this fix — it drains once, at the end, which is the case the finding is
//! about. Anything that has to outlive a turn is held as a `Retained`, which is
//! `+1` and owes a pool nothing; `Machinery` is made of exactly those, which is
//! why it can be built inside one pool and used inside the next.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AnyThread, ClassType, DefinedClass, define_class, msg_send, sel};
use objc2_av_foundation::{
    AVAudioTimePitchAlgorithmTimeDomain, AVError, AVMediaTypeAudio, AVMediaTypeVideo, AVPlayer,
    AVPlayerActionAtItemEnd, AVPlayerItem, AVPlayerItemDidPlayToEndTimeNotification,
    AVPlayerItemStatus, AVPlayerItemVideoOutput, AVURLAsset,
};
use objc2_core_foundation::CGSize;
use objc2_core_media::{CMTime, CMTimeFlags, kCMTimeZero};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSString};

use super::engine::{
    EngineError, EngineState, FRAME_POLL_INTERVAL, Frame, FrameCost, IDLE_POLL_INTERVAL,
    LedgerEntry, OPEN_BUDGET, SHUTDOWN_BUDGET, note_engine_shut_down,
};
use super::macos_video::displayed_size;
use crate::macos_files::file_url;

/// **The timescale an arbitrary number of seconds is asked for in.**
///
/// Six hundred, which is QuickTime's own and the least common multiple of the
/// frame rates a file is likely to be written at (24, 25, 30, 60), so a seek to
/// a whole number of frames at any of them is exact rather than rounded. It is
/// used only for a [`Engine::seek`] the caller expressed in seconds; every time
/// that comes out of the file keeps the file's own timescale.
const SEEK_TIMESCALE: i32 = 600;

// ── the handle ──────────────────────────────────────────────────────────────

/// **A video that is loaded, and a thread that is looking after it.**
///
/// The handle is ordinary Rust: no Objective-C object is reachable through it,
/// so it may be held by a pane, moved between them, and dropped on any thread.
/// Every method is a message to the engine thread except [`Self::state`],
/// [`Self::frame`], [`Self::standing_frame`] and [`Self::frame_cost`], which
/// read what that thread last published.
///
/// **Commands are fire-and-forget**, for the Windows arm's reason: `play` on an
/// engine whose thread has already stopped is not an error a caller can do
/// anything about — the state will say so on the next read — and a `Result` on
/// every verb would put error handling on six call sites that would all discard
/// it.
pub struct Engine {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
    source: PathBuf,
    seen_generation: u64,
    /// When [`Engine::open`] returned — which, since it waits for nothing, is
    /// also when the engine thread was asked to build one. [`Self::state`]
    /// measures [`OPEN_BUDGET`] from here.
    opened_at: Instant,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Engine")
            .field("source", &self.source)
            .field("state", &self.state())
            .finish()
    }
}

impl Engine {
    /// **Open `path` and start a thread to play it on.**
    ///
    /// Returns when the engine has been *asked for*, not when it exists and not
    /// when the video has loaded — the Windows arm's contract, and the reason it
    /// is that contract is §7.42's rule about the drawing thread: a window does
    /// not have a hundred milliseconds to give a button press. A caller that
    /// needs the size or the length watches [`EngineState::ready`]; a caller
    /// that needs to know whether the file can be played at all watches
    /// [`EngineState::error`].
    ///
    /// # Where it may be called from
    ///
    /// Any thread, including the window's, and this function **waits for
    /// nothing**. It allocates a channel and starts a thread; the `AVURLAsset`,
    /// the item, the player and the video output are all on the far side of that
    /// thread's first instruction.
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        let shared = Arc::new(Shared::default());
        let (commands, inbox) = mpsc::channel();
        let source = path.to_path_buf();
        let url = source.clone();
        let thread = {
            let shared = Arc::clone(&shared);
            let commands = commands.clone();
            std::thread::Builder::new()
                .name("folio-video-engine".to_owned())
                .spawn(move || {
                    run(&url, &shared, &commands, &inbox);
                    // Last of all, and read by `Engine::shutdown` to know
                    // whether joining this thread will return — see there.
                    shared.stopped.store(true, Ordering::Release);
                })
                // The one thing that can still fail here, and it fails without
                // having started anything: a process out of thread handles.
                .map_err(|_| EngineError::Other("no thread for the engine"))?
        };
        Ok(Self {
            commands,
            shared,
            thread: Some(thread),
            source,
            seen_generation: 0,
            opened_at: Instant::now(),
        })
    }

    /// The file this engine was opened on — the spelling the caller gave, not
    /// the disk's, because every surface that compares it holds the same one.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Everything knowable about this engine at this instant. See
    /// [`EngineState`].
    ///
    /// **[`OPEN_BUDGET`] is spent here**, and it is the only place it is spent,
    /// for the Windows arm's reason: [`Self::open`] waits for nothing, so nobody
    /// is left holding a timer when an engine fails to come into being. Past the
    /// budget with nothing built and nothing said, the answer is an error a
    /// surface already knows how to print.
    #[must_use]
    pub fn state(&self) -> EngineState {
        let state = *self
            .shared
            .state
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        if state.error.is_none()
            && !self.shared.built.load(Ordering::Acquire)
            && self.opened_at.elapsed() > OPEN_BUDGET
        {
            return EngineState {
                error: Some(EngineError::Other("the engine did not come up")),
                ..state
            };
        }
        state
    }

    /// **The newest picture, if it is newer than the last one this handle
    /// returned**, and `None` otherwise.
    ///
    /// `&mut self` is the whole contract, and it is the Windows arm's: "newer
    /// than the last one" is a fact about *this handle*, so one caller draining
    /// frames cannot make another caller's `frame()` answer nothing.
    ///
    /// Frames are **not queued**. The engine thread keeps the most recent one
    /// and drops the one before it, because a video is a clock: a caller that
    /// fell behind wants the picture that is due now, not the four it missed.
    pub fn frame(&mut self) -> Option<Frame> {
        if self.shared.generation.load(Ordering::Acquire) <= self.seen_generation {
            return None;
        }
        let frame = self
            .shared
            .frame
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()?;
        if frame.generation <= self.seen_generation {
            return None;
        }
        self.seen_generation = frame.generation;
        Some(frame)
    }

    /// The most recent picture **whether or not this handle has seen it** — for
    /// a redraw that was caused by something other than the video, which must
    /// still paint the frame that is standing.
    ///
    /// It is also what makes an ended video end without a stuck frame: nothing
    /// in this module ever clears it, so the picture on the glass when the clock
    /// stops is the last one the file had.
    #[must_use]
    pub fn standing_frame(&self) -> Option<Frame> {
        self.shared
            .frame
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }

    /// Where the last frame's time went; see [`FrameCost`].
    #[must_use]
    pub fn frame_cost(&self) -> FrameCost {
        *self
            .shared
            .cost
            .lock()
            .unwrap_or_else(|held| held.into_inner())
    }

    pub fn play(&self) {
        let _ = self.commands.send(Command::Play);
    }

    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }

    /// Move the playhead. Out-of-range values are the player's business to
    /// clamp, exactly as they are `HTMLMediaElement.currentTime`'s.
    pub fn seek(&self, secs: f64) {
        let _ = self.commands.send(Command::Seek(secs));
    }

    pub fn set_rate(&self, rate: f64) {
        let _ = self.commands.send(Command::Rate(rate));
    }

    pub fn set_muted(&self, muted: bool) {
        let _ = self.commands.send(Command::Muted(muted));
    }

    pub fn set_volume(&self, volume: f64) {
        let _ = self.commands.send(Command::Volume(volume));
    }

    /// **Wait until the metadata has arrived, or until `budget` runs out.**
    ///
    /// # Where it may be called from
    ///
    /// **Never the thread that draws.** A window that waited here would be a
    /// window frozen for as long as a container takes to open. The product polls
    /// [`EngineState::ready`]; this exists for a test, and for a caller already
    /// on a worker.
    pub fn wait_for_metadata(&self, budget: Duration) -> bool {
        let deadline = Instant::now() + budget;
        loop {
            let state = self.state();
            if state.ready || state.error.is_some() {
                return state.ready;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// **Stop, release the player and join its thread**, now rather than at
    /// drop.
    ///
    /// The verb a pane calls when it closes or is handed a different file. It is
    /// idempotent and [`Drop`] calls it, so a caller that forgets is not a
    /// caller that leaks.
    ///
    /// # The join is bounded
    ///
    /// The Windows arm's review row R2-19, and the reason is the same on this
    /// platform: a pane closes on the window's own thread, and the thread it is
    /// closing may be inside a decoder. So the thread is asked to stop and this
    /// waits [`SHUTDOWN_BUDGET`] for it to say it has — the flag it sets as its
    /// very last act, which is what makes the `join` after it a formality that
    /// returns at once. A thread that has not said so by then is let go rather
    /// than waited on, and its engine is still on the ledger, which is the
    /// truth.
    pub fn shutdown(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        let Some(thread) = self.thread.take() else {
            return;
        };
        let deadline = Instant::now() + SHUTDOWN_BUDGET;
        while !self.shared.stopped.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                eprintln!(
                    "video: an engine thread was still running {SHUTDOWN_BUDGET:?} \
                     after it was told to stop; leaving it to finish"
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = thread.join();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// What the handle publishes to and the thread publishes from. Three separate
/// locks rather than one, because the three are read at three different rates: a
/// renderer takes `frame` sixty times a second, a layout takes `state` when
/// something moves, and `cost` is read by a human.
#[derive(Default)]
struct Shared {
    state: Mutex<EngineState>,
    frame: Mutex<Option<Frame>>,
    cost: Mutex<FrameCost>,
    /// Read before the `frame` lock is taken, so the overwhelmingly common
    /// "nothing new" answer costs one atomic load and no contention with the
    /// thread that is writing the next picture.
    generation: std::sync::atomic::AtomicU64,
    /// **Whether the engine thread got as far as having a player.** Set once,
    /// immediately before the pump starts; [`Engine::state`] reads it to decide
    /// whether [`OPEN_BUDGET`] has been missed.
    built: std::sync::atomic::AtomicBool,
    /// **Whether the engine thread has left.** Set once, as the thread's last
    /// act, and read by [`Engine::shutdown`] so that its join is a bounded wait.
    stopped: std::sync::atomic::AtomicBool,
}

/// One thing for the engine thread to do. The twin of the Windows arm's, with
/// its `Event(u32)` replaced by the one event this platform raises that this
/// module cannot re-read off an object.
enum Command {
    Play,
    Pause,
    Seek(f64),
    Rate(f64),
    Muted(bool),
    Volume(f64),
    /// The item said it had played to its end, forwarded off whatever thread
    /// AVFoundation posted the notification on.
    Ended,
    Shutdown,
}

// ── the engine thread ───────────────────────────────────────────────────────

/// **Everything a player costs, on the player's own thread.**
///
/// Nothing is reported back through a channel, because nobody is waiting on
/// one: a failure at any point is written into the shared [`EngineState`] as its
/// `error`, which is the same slot a codec refusing a file a minute in writes to
/// and the same slot every surface already reads.
///
/// **The outer autorelease pool is here** (RA-5, and the module note's last
/// section). This is a thread this file started, so nothing has made a pool on
/// it; one is made before the first message is sent and drained after the last
/// one, which covers everything [`Machinery::build`] and [`Machinery::stop`]
/// send. It is not what covers the pump — that has a pool of its own, per turn,
/// because a pool that drains when playback ends is a pool that holds
/// everything playback made.
fn run(
    path: &Path,
    shared: &Arc<Shared>,
    commands: &mpsc::Sender<Command>,
    inbox: &mpsc::Receiver<Command>,
) {
    autoreleasepool(|_pool| match Machinery::build(path, commands) {
        Ok(mut machinery) => {
            // Before the pump and after the player: this is what stops
            // `Engine::state` charging a working engine with having missed
            // `OPEN_BUDGET`.
            shared.built.store(true, Ordering::Release);
            machinery.pump(shared, inbox);
            machinery.stop();
            note_engine_shut_down();
        }
        Err(error) => publish_failure(shared, error),
    });
}

/// An engine that never came into being, said in the one place a caller looks.
fn publish_failure(shared: &Arc<Shared>, error: EngineError) {
    let mut state = shared.state.lock().unwrap_or_else(|held| held.into_inner());
    if state.error.is_none() {
        state.error = Some(error);
    }
}

/// Everything the engine thread owns. It never leaves that thread — the type is
/// deliberately not `Send`, because two of the objects in it are annotated for
/// Swift's main actor and this module's whole answer to that is that they are
/// touched from one thread and one thread only.
struct Machinery {
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
    /// The notification observer, removed in [`Self::stop`] before the item it
    /// is scoped to is released.
    end_watch: Option<Retained<EndWatch>>,
    /// What the file says about itself, read once off the asset the way
    /// [`super::macos_video`] reads it, because these two facts are answerable
    /// before the player is ready and a caller that has to lay a rectangle out
    /// wants them then.
    declared_secs: Option<f64>,
    native_size: Option<(u32, u32)>,
    has_video: bool,
    has_audio: bool,
    /// **The rate the reader asked for, which is not `AVPlayer.rate`.** On this
    /// platform `rate` is the pause control as well as the speed — `pause` is
    /// `rate = 0` — so a player paused at one-and-a-half speed reads back zero
    /// and a caller that believed it would draw `0×` on the bar. The Windows
    /// arm's `SetPlaybackRate` is a separate property from `Pause`, and this is
    /// what makes the two arms answer [`EngineState::rate`] the same way.
    wanted_rate: f64,
    /// Whether the item has posted its end since the last seek or restart. See
    /// the module note on why this one fact is recorded rather than re-read.
    ended: bool,
    generation: u64,
    error: Option<EngineError>,
}

impl Machinery {
    #[expect(
        deprecated,
        reason = "the synchronous track accessor is what a thread of one's own wants; see \
                  `macos_video.rs`'s note, which this file inherits"
    )]
    fn build(path: &Path, commands: &mpsc::Sender<Command>) -> Result<Self, EngineError> {
        let url = file_url(path, false).map_err(|_| EngineError::Other("the path has no URL"))?;
        // SAFETY: every call below is an Objective-C message to an object this
        // function created and holds, on the thread that created it. The statics
        // read here — `AVMediaTypeVideo`, `AVMediaTypeAudio`, the pixel-format
        // key, the notification name and the pitch algorithm — are framework
        // constants that live for the process. `alloc_off_the_window_thread` is
        // the module note's own subject and carries its argument there.
        unsafe {
            // An `AVURLAsset` does not open anything here: it is a name, and the
            // file behind it is read when something is asked of it. What is
            // asked of it first is its track list, which is where a file that is
            // not a video is turned away.
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            let video = AVMediaTypeVideo.map(|kind| asset.tracksWithMediaType(kind));
            let audio = AVMediaTypeAudio.map(|kind| asset.tracksWithMediaType(kind));
            let has_video = video.as_ref().is_some_and(|tracks| !tracks.is_empty());
            let has_audio = audio.as_ref().is_some_and(|tracks| !tracks.is_empty());
            if !has_video && !has_audio {
                // A text file with a video's name on it, a truncated download, a
                // container this machine has no source for. The same sentence
                // the other arm's `MEDIA_ERR_SRC_NOT_SUPPORTED` is, said as
                // early as it can be said rather than waited for.
                return Err(EngineError::Unsupported);
            }
            let native_size = video
                .as_ref()
                .and_then(|tracks| tracks.firstObject())
                .and_then(|track| displayed_size(track.naturalSize(), track.preferredTransform()));
            let declared_secs = declared_seconds(asset.duration());

            let item: Retained<AVPlayerItem> =
                AVPlayerItem::initWithAsset(alloc_off_the_window_thread::<AVPlayerItem>(), &asset);
            // **Pitch is kept, which is what the other arm's rate does.**
            // `SetPlaybackRate` on a media engine plays faster without turning
            // a voice into a chipmunk; the time-domain algorithm is this
            // platform's spelling of that, and it is what macOS 12 and later
            // already default to. Named anyway, because a default is not a
            // decision anybody wrote down.
            if let Some(algorithm) = AVAudioTimePitchAlgorithmTimeDomain {
                item.setAudioTimePitchAlgorithm(algorithm);
            }
            let output = video_output();
            item.addOutput(&output);

            let player: Retained<AVPlayer> = AVPlayer::initWithPlayerItem(
                alloc_off_the_window_thread::<AVPlayer>(),
                Some(&item),
            );
            // **The last picture stands.** `Pause` rather than `None` is what
            // leaves the final frame on the item when the clock stops; `None`
            // would leave the player with no current item and this module with
            // nothing to answer a redraw with.
            player.setActionAtItemEnd(AVPlayerActionAtItemEnd::Pause);
            // **A local file does not need the stall heuristic**, and with it on
            // `play` can answer `WaitingToPlayAtSpecifiedRate` for a file that
            // is already entirely on the disk — which this module would have to
            // report as "not playing" to a bar that had just been pressed.
            player.setAutomaticallyWaitsToMinimizeStalling(false);

            // A ledger entry from the moment there is a player, closed by
            // whichever of the two happens: `kept` hands it to the machinery,
            // and anything else — an error below, a panic — closes it here. The
            // Windows arm's review row R2-19, and the defect it was written for
            // is the same one: a fallible step between "an engine exists" and
            // "something will stop it" left the count added for ever.
            let ledger = LedgerEntry::opened();
            let end_watch = EndWatch::watching(&item, commands);
            ledger.kept();
            Ok(Self {
                player,
                item,
                output,
                end_watch,
                declared_secs,
                native_size,
                has_video,
                has_audio,
                wanted_rate: 1.0,
                ended: false,
                generation: 0,
                error: None,
            })
        }
    }

    /// The loop: answer commands, publish state, and take a picture when there
    /// is one. The Windows arm's `pump`, line for line.
    ///
    /// **One autorelease pool per turn** (RA-5). The loop is here and the work
    /// is in [`Self::one_turn`] for exactly that: the pool has to be opened and
    /// drained inside the loop, because a pool opened outside it drains when
    /// the video is closed and holds every object the frameworks autoreleased
    /// in between — which for a preview left open is the whole preview. The
    /// only thing carried from one turn to the next is `self`, and everything
    /// of AVFoundation's in it is a `Retained`.
    fn pump(&mut self, shared: &Arc<Shared>, inbox: &mpsc::Receiver<Command>) {
        while autoreleasepool(|_pool| self.one_turn(shared, inbox)) {
            // The turn's pool is drained on the way back out of that call, and
            // this is the line that says so: the loop body is deliberately
            // empty, because everything a turn does has to happen inside the
            // pool rather than beside it.
        }
    }

    /// One turn of [`Self::pump`] — `false` when the engine has been told to
    /// stop, or when every handle to it has gone.
    fn one_turn(&mut self, shared: &Arc<Shared>, inbox: &mpsc::Receiver<Command>) -> bool {
        let state = self.publish_state(shared);
        // A picture is due while the clock is running, and once more after
        // anything else — a seek while paused draws a new frame, and so does
        // the load that first produces one.
        let wait = if state.playing {
            FRAME_POLL_INTERVAL
        } else {
            IDLE_POLL_INTERVAL
        };
        match inbox.recv_timeout(wait) {
            Ok(Command::Shutdown) => return false,
            Ok(command) => {
                self.apply(command);
                // Drain whatever else is already waiting before spending a
                // poll on it.
                while let Ok(next) = inbox.try_recv() {
                    if matches!(next, Command::Shutdown) {
                        return false;
                    }
                    self.apply(next);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // Every handle has gone without saying so — a panic between the
            // `Engine` being made and being dropped. Same ending.
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
        self.take_frame(shared);
        true
    }

    fn apply(&mut self, command: Command) {
        // SAFETY: Objective-C messages to this thread's own player and item; see
        // the module note on which thread that is and why.
        unsafe {
            match command {
                Command::Play => {
                    // **A play on an ended video starts it again**, which is
                    // `HTMLMediaElement.play`'s own rule and therefore the other
                    // arm's: an engine whose playhead is at the end has nowhere
                    // to go, and a reader pressing ▶ on it means *again*.
                    if self.ended {
                        self.seek_to(0.0);
                        self.ended = false;
                    }
                    self.player.setRate(self.wanted_rate as f32);
                }
                Command::Pause => self.player.pause(),
                Command::Seek(secs) => {
                    self.seek_to(secs);
                    if self.seek_landed_before_the_end(secs) {
                        self.ended = false;
                    }
                }
                Command::Rate(rate) => {
                    self.wanted_rate = rate;
                    // `defaultRate` is what a later `play` will use; `rate` is
                    // set too, but only while the clock is already running —
                    // setting it on a paused player is how you start one, and
                    // "faster" is not "play".
                    self.player.setDefaultRate(rate as f32);
                    if self.player.rate() != 0.0 {
                        self.player.setRate(rate as f32);
                    }
                }
                Command::Muted(muted) => self.player.setMuted(muted),
                Command::Volume(volume) => {
                    self.player.setVolume(volume.clamp(0.0, 1.0) as f32);
                }
                Command::Ended => self.ended = true,
                Command::Shutdown => {}
            }
        }
    }

    /// **An exact seek**, which is what `SetCurrentPosition` is on the other
    /// platform: both tolerances are zero, so the player decodes forward from
    /// the key frame before the target rather than landing on the key frame
    /// itself. A reader dragging the head to a moment wants that moment.
    ///
    /// # SAFETY
    ///
    /// Called from [`Self::apply`] and [`Self::build`]'s successors, on the
    /// engine thread, with a player this struct owns.
    unsafe fn seek_to(&self, secs: f64) {
        // SAFETY: `CMTime::with_seconds` is arithmetic, `kCMTimeZero` is a
        // framework constant, and the message goes to this thread's own player.
        unsafe {
            let at = CMTime::with_seconds(secs.max(0.0), SEEK_TIMESCALE);
            self.player
                .seekToTime_toleranceBefore_toleranceAfter(at, kCMTimeZero, kCMTimeZero);
        }
    }

    /// Whether a seek to `secs` puts the playhead somewhere the file still has
    /// pictures — which is what un-ends an ended video, and what a seek to the
    /// very end does not.
    fn seek_landed_before_the_end(&self, secs: f64) -> bool {
        self.declared_secs.is_none_or(|length| secs < length)
    }

    fn publish_state(&mut self, shared: &Arc<Shared>) -> EngineState {
        // SAFETY: Objective-C messages to this thread's own player and item.
        let state = unsafe {
            let status = self.item.status();
            if status == AVPlayerItemStatus::Failed && self.error.is_none() {
                self.error = Some(
                    self.item
                        .error()
                        .map_or(EngineError::Decode, |error| engine_error_of(error.code())),
                );
            }
            // The length: the item's once it has one, and the asset's before
            // that. Both are the same number for a local file, and the second is
            // answerable a great deal earlier.
            let duration_secs = declared_seconds(self.item.duration()).or(self.declared_secs);
            // The size: the item's presentation size once it has one, which is
            // already the *displayed* size — a portrait capture stored landscape
            // with a quarter turn beside it reads the way a reader sees it — and
            // the track's own turned size before that.
            let presentation = self.item.presentationSize();
            let natural_size = size_of(presentation).or(self.native_size);
            let rate = self.player.rate();
            EngineState {
                duration_secs,
                position_secs: seconds_or_zero(self.player.currentTime()),
                // **Not "a frame changed recently".** A video paused on its last
                // picture is `false` here and still has a picture on the glass.
                playing: rate != 0.0 && !self.ended,
                natural_size,
                ended: self.ended,
                error: self.error,
                muted: self.player.isMuted(),
                volume: f64::from(self.player.volume()),
                rate: self.wanted_rate,
                // **The size, not a status** — the Windows arm's spelling, and
                // on this platform it is the only one that is true.
                //
                // `ready` means "the metadata has arrived": the moment
                // `natural_size` and `duration_secs` are answerable and a layout
                // can be solved. Both are answered off the **asset**, which
                // `macos_video.rs` already reads synchronously, so both are true
                // from this engine's first published state.
                //
                // `AVPlayerItem.status` was the first spelling of this and it is
                // wrong here, measured on the venue machine 2026-09-12: in a
                // process whose main queue nobody drains it stays
                // `AVPlayerItemStatusUnknown` for ever, because that property is
                // one of the "changes that occur dynamically during playback"
                // the class's own header says are serialized onto a dispatch
                // queue that is the main queue by default. Spelling `ready` as
                // the status would make this flag a statement about the caller's
                // run loop rather than about the file — see §13.35 ⑤.
                ready: natural_size.is_some() || self.has_audio,
                has_video: self.has_video,
                has_audio: self.has_audio,
            }
        };
        *shared.state.lock().unwrap_or_else(|held| held.into_inner()) = state;
        state
    }

    /// Ask whether there is a new picture and, if there is, bring it back.
    ///
    /// The time asked for is the **player's own current time** rather than a
    /// host time turned into an item time. Apple's sample does the latter
    /// because a `CADisplayLink` hands it the timestamp of a vertical blank that
    /// has not happened yet; this module has no such timestamp and no such
    /// clock — it is asked for the picture that is due *now*, and `currentTime`
    /// is what now means on the item's own timeline. It is also what makes a
    /// seek on a paused player produce a frame: the playhead moved, so the
    /// question changed.
    fn take_frame(&mut self, shared: &Arc<Shared>) {
        // SAFETY: Objective-C messages to this thread's own player and output,
        // and Core Video calls on a pixel buffer this function owns for as long
        // as it is locked. Every read out of the buffer is bounded by the row
        // count and the row width the buffer itself declared.
        unsafe {
            let at = self.player.currentTime();
            if !self.output.hasNewPixelBufferForItemTime(at) {
                return;
            }
            let mut cost = FrameCost::default();
            let started = Instant::now();
            let Some(buffer) = self
                .output
                .copyPixelBufferForItemTime_itemTimeForDisplay(at, std::ptr::null_mut())
            else {
                // A null buffer is the documented way of saying that nothing
                // should be displayed for this time. It is not a failure and it
                // is not a new picture.
                return;
            };
            cost.transfer = started.elapsed();

            let started = Instant::now();
            let locked =
                CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::ReadOnly) == 0;
            cost.readback = started.elapsed();
            if !locked {
                return;
            }
            let started = Instant::now();
            let bgra = self.tight_rows(&buffer);
            let _ = CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::ReadOnly);
            cost.copy = started.elapsed();

            let Some((bgra, width, height)) = bgra else {
                return;
            };
            self.generation += 1;
            cost.frames = self.generation;
            *shared.frame.lock().unwrap_or_else(|held| held.into_inner()) = Some(Frame {
                bgra: Arc::from(bgra.into_boxed_slice()),
                width,
                height,
                generation: self.generation,
            });
            *shared.cost.lock().unwrap_or_else(|held| held.into_inner()) = cost;
            shared.generation.store(self.generation, Ordering::Release);
        }
    }

    /// The rows of a locked pixel buffer, packed at `width * 4`.
    ///
    /// The format is **checked rather than trusted**: the output was asked for
    /// `kCVPixelFormatType_32BGRA` and a buffer that is anything else — a planar
    /// `420v` from a decoder that refused the request, an `f16` from an HDR
    /// path — would be read as if its rows were four bytes a pixel, which is a
    /// picture of noise rather than a wrong colour.
    ///
    /// # SAFETY
    ///
    /// Called from [`Self::take_frame`] with a buffer whose base address is
    /// locked for the length of the call.
    unsafe fn tight_rows(&self, buffer: &CVPixelBuffer) -> Option<(Vec<u8>, u32, u32)> {
        // SAFETY: six Core Video accessors on a locked buffer, and one read of
        // `stride * height` bytes out of the store they describe.
        unsafe {
            if CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_32BGRA {
                return None;
            }
            let width = u32::try_from(CVPixelBufferGetWidth(buffer)).ok()?;
            let height = u32::try_from(CVPixelBufferGetHeight(buffer)).ok()?;
            if width == 0 || height == 0 {
                return None;
            }
            let stride = CVPixelBufferGetBytesPerRow(buffer);
            let store = CVPixelBufferGetBaseAddress(buffer);
            if store.is_null() {
                return None;
            }
            let rows = std::slice::from_raw_parts(
                store.cast::<u8>(),
                stride.checked_mul(height as usize)?,
            );
            copy_tight(rows, stride, width, height).map(|bgra| (bgra, width, height))
        }
    }

    /// **Everything given back, in the reverse order it was taken.**
    ///
    /// The observer first, because a notification centre holds its observers
    /// unretained and one left behind is a pointer to a freed object the next
    /// time anything posts; then the output, because an item that still has one
    /// keeps a decoder attached to it; then the player is paused and emptied,
    /// which is what releases the item and the asset behind it.
    fn stop(&mut self) {
        // SAFETY: Objective-C messages to this thread's own objects. Idempotent:
        // the watch is taken out of its slot, and the other three are all
        // harmless to say twice — which is what lets [`Drop`] be the belt under
        // the explicit call.
        unsafe {
            if let Some(watch) = self.end_watch.take() {
                watch.stop_watching();
            }
            self.item.removeOutput(&self.output);
            self.player.pause();
            self.player.replaceCurrentItemWithPlayerItem(None);
        }
    }
}

impl Drop for Machinery {
    /// **The observer comes off even when nobody called [`Self::stop`].**
    ///
    /// `run` calls it on every ordinary path; this is the path where the pump
    /// unwound. A notification centre holds its observers **unretained**, so a
    /// `Machinery` that was dropped without this would leave the centre pointing
    /// at freed memory — a crash on the next `AVPlayerItemDidPlayToEndTimeNotification`
    /// anywhere in the process, which is to say in a *different* video's engine.
    /// The Windows arm has no twin for this because a COM callback is
    /// reference-counted and releasing the engine unhooks it.
    fn drop(&mut self) {
        self.stop();
    }
}

// ── the pieces ──────────────────────────────────────────────────────────────

/// **Allocate a class `objc2` believes belongs to the main thread, from the
/// thread this engine owns.**
///
/// The whole of the module note's fourth section in one call. `AVPlayer` and
/// `AVPlayerItem` carry `NS_SWIFT_UI_ACTOR` in the SDK headers, so `objc2`
/// generates them as `MainThreadOnly` and every typed constructor asks for a
/// `MainThreadMarker` this thread has not got and must not invent. `alloc` is
/// the one message that marker guards, and this is that message without it.
///
/// # SAFETY
///
/// The caller must keep every object allocated here on one thread for its whole
/// life — which is what [`Machinery`] is — and must not hand it to anything that
/// would ask it for a `MainThreadMarker`.
unsafe fn alloc_off_the_window_thread<T: ClassType>() -> Allocated<T> {
    // SAFETY: `alloc` on a class object, which is the `alloc` family's contract:
    // the receiver is a class and the return type is `Allocated<T>`.
    unsafe { msg_send![T::class(), alloc] }
}

/// **The video output, asked for the byte order this window draws in.**
///
/// `kCVPixelFormatType_32BGRA` is B, G, R, A in memory, which is [`Frame`]'s
/// promise and the swapchain's format, so the whole path from decoder to glass
/// reorders nothing. The attributes go in as a dictionary because that is the
/// only shape this initializer takes, and the key is Core Video's own constant
/// rather than the string in it.
///
/// # SAFETY
///
/// Called from [`Machinery::build`] on the engine thread. The key read here is a
/// framework constant that lives for the process.
unsafe fn video_output() -> Retained<AVPlayerItemVideoOutput> {
    // SAFETY: a framework constant, and one Objective-C initializer on a fresh
    // allocation. `CFString` and `NSString` are toll-free bridged — Apple
    // documents the two as interchangeable wherever either is taken — which is
    // what lets a Core Video key be a key of an `NSDictionary`.
    unsafe {
        let key: &NSString = &*std::ptr::from_ref(kCVPixelBufferPixelFormatTypeKey).cast();
        let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
        let value: &AnyObject = &format;
        let attributes: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[key], &[value]);
        AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
            AVPlayerItemVideoOutput::alloc(),
            Some(&attributes),
        )
    }
}

/// **Copy `height` rows of `width` pixels out of a buffer whose rows are
/// `stride` bytes apart, into a buffer whose rows are `width * 4` bytes apart.**
///
/// The one function in this file that needs no Mac, and the one that would fail
/// silently if it were wrong: a `CVPixelBuffer`'s `bytesPerRow` is padded to the
/// decoder's alignment — sixteen pixels is usual, so a 160-wide frame arrives
/// 704 bytes a row rather than 640 — and a copy that took `width * height * 4`
/// contiguous bytes would shear every row after the first by the padding of the
/// one before it.
///
/// `None` for a stride narrower than a row, which is a buffer that does not
/// describe the picture it claims to.
fn copy_tight(rows: &[u8], stride: usize, width: u32, height: u32) -> Option<Vec<u8>> {
    let row_bytes = (width as usize).checked_mul(4)?;
    let rows_wanted = height as usize;
    if stride < row_bytes || rows.len() < stride.checked_mul(rows_wanted)? {
        return None;
    }
    let mut tight = vec![0_u8; row_bytes.checked_mul(rows_wanted)?];
    for row in 0..rows_wanted {
        let from = row * stride;
        tight[row * row_bytes..(row + 1) * row_bytes]
            .copy_from_slice(&rows[from..from + row_bytes]);
    }
    Some(tight)
}

/// **A `CMTime` as a number of seconds, or `None` when it is not a length.**
///
/// A live source, a stream still being written and a container with no index all
/// answer with something that is not a number — invalid, indefinite, or one of
/// the two infinities — and nothing is what a caller then says. `macos_video.rs`
/// makes the same judgement for the same reason; it is written twice because the
/// two files answer it in different units.
fn declared_seconds(time: CMTime) -> Option<f64> {
    if !time.flags.contains(CMTimeFlags::Valid)
        || time.flags.intersects(CMTimeFlags::ImpliedValueFlagsMask)
        || time.timescale <= 0
        || time.value <= 0
    {
        return None;
    }
    // SAFETY: a `CMTime` this function has already established is valid,
    // numeric and positively scaled; `CMTimeGetSeconds` is a division.
    let seconds = unsafe { time.seconds() };
    seconds.is_finite().then_some(seconds)
}

/// The playhead, in seconds, with every shape of "no answer" spelled `0.0` —
/// which is where a playhead that is nowhere is.
fn seconds_or_zero(time: CMTime) -> f64 {
    declared_seconds(time).unwrap_or(0.0)
}

/// A presentation size as whole pixels, or `None` for the zero size an item
/// answers with before it has loaded.
fn size_of(size: CGSize) -> Option<(u32, u32)> {
    let (width, height) = (size.width.round(), size.height.round());
    (width >= 1.0 && height >= 1.0 && width <= f64::from(u32::MAX) && height <= f64::from(u32::MAX))
        .then_some((width as u32, height as u32))
}

/// **What one `AVFoundationErrorDomain` code means to a reader.**
///
/// The twin of the Windows arm's `EngineError::of_code`, and it collapses to the
/// same place for the same reason: a file this machine cannot open is one
/// sentence however the platform spells the refusal, and the default is that
/// sentence rather than a number nobody can act on.
fn engine_error_of(code: isize) -> EngineError {
    match AVError(code) {
        AVError::DecodeFailed => EngineError::Decode,
        AVError::ContentIsProtected => EngineError::Other("the content is protected"),
        AVError::NoLongerPlayable => EngineError::Other("the source stopped being playable"),
        // `FileFormatNotRecognized`, `FileFailedToParse`, `InvalidSourceMedia`
        // and anything the platform invents later: this is not a file this
        // machine can open.
        _ => EngineError::Unsupported,
    }
}

// ── the one notification ────────────────────────────────────────────────────

/// **The one object the notification centre calls back into**, and the whole of
/// what it may do.
///
/// It arrives on whatever thread AVFoundation posted from — never a window's,
/// never necessarily the engine thread's — so the only safe thing to do with it
/// is to say that something happened somewhere else. That is exactly what it
/// does: one `send`. It is `video/engine.rs`'s `Notify` with a notification
/// centre where the COM callback was.
struct EndWatchIvars {
    /// A `Sender` is `Send` and not `Sync`, and a notification may be posted
    /// from two threads at once; the lock is what makes the second one wait
    /// rather than a data race.
    commands: Mutex<mpsc::Sender<Command>>,
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - This class does not implement `Drop`; its ivars do, and the macro's
    //   generated `dealloc` runs them.
    #[unsafe(super(NSObject))]
    #[name = "FolioVideoEndWatch"]
    #[ivars = EndWatchIvars]
    struct EndWatch;

    impl EndWatch {
        /// A name of Folio's own, prefixed, because it is added to a class this
        /// program defines and a plain `ended:` would be a selector the
        /// Objective-C runtime shares with everything else that ever thought of
        /// the word.
        #[unsafe(method(folioVideoDidPlayToEnd:))]
        fn did_play_to_end(&self, _notification: &NSNotification) {
            if let Ok(commands) = self.ivars().commands.lock() {
                // A closed channel is an engine whose thread has already
                // stopped, which is an ordinary ending and not an error.
                let _ = commands.send(Command::Ended);
            }
        }
    }

    unsafe impl NSObjectProtocol for EndWatch {}
);

impl EndWatch {
    /// Register for **this item's** end and nothing else's. A process may be
    /// playing a video on three surfaces, and an observer scoped to `nil` would
    /// end all three when one of them finished.
    fn watching(item: &AVPlayerItem, commands: &mpsc::Sender<Command>) -> Option<Retained<Self>> {
        let this = Self::alloc().set_ivars(EndWatchIvars {
            commands: Mutex::new(commands.clone()),
        });
        // SAFETY: `NSObject`'s designated initializer on a fresh allocation
        // whose ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let observer: &AnyObject = &this;
        let subject: &AnyObject = item;
        // SAFETY: the selector is one this class defines above and takes one
        // argument, which is what a notification handler is; the name is a
        // framework constant that lives for the process. The centre holds this
        // observer unretained, which is why `stop_watching` exists and why
        // `Machinery::stop` calls it before the item goes.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                observer,
                sel!(folioVideoDidPlayToEnd:),
                Some(AVPlayerItemDidPlayToEndTimeNotification),
                Some(subject),
            );
        }
        Some(this)
    }

    /// Unregister, before the item this was scoped to is released.
    ///
    /// # SAFETY
    ///
    /// Called once, from [`Machinery::stop`], on the engine thread.
    unsafe fn stop_watching(&self) {
        let observer: &AnyObject = self;
        // SAFETY: one message to the default centre, naming this observer.
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver(observer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — **a padded row is copied by its width and not by its stride.**
    ///
    /// The one assertion in this file that needs no video and no Mac, and the
    /// defect it is for is invisible in a unit test of anything else: a
    /// `CVPixelBuffer` 160 pixels wide arrives with 704-byte rows on this
    /// platform's decoders, and a copy that walked the store contiguously would
    /// put 16 pixels of another row at the end of every row and shear the whole
    /// picture one notch further each line.
    ///
    /// MUTATION: copy `row_bytes` from `row * row_bytes` instead of
    /// `row * stride` and the second row comes back wrong.
    #[test]
    fn a_padded_row_is_copied_by_its_width_and_not_by_its_stride() {
        // Three rows of two pixels, in a store whose rows are three pixels
        // apart: the third pixel of each row is padding and must not survive.
        let width = 2_u32;
        let height = 3_u32;
        let stride = 12_usize;
        let mut store = vec![0_u8; stride * height as usize];
        for row in 0..height as usize {
            for byte in 0..stride {
                store[row * stride + byte] = if byte < 8 {
                    (row * 8 + byte) as u8
                } else {
                    0xFF
                };
            }
        }
        let tight = copy_tight(&store, stride, width, height).expect("the rows are copied");
        assert_eq!(tight.len(), 2 * 3 * 4);
        assert_eq!(
            tight,
            (0..24_u8).collect::<Vec<_>>(),
            "the padding of one row was carried into the next"
        );
    }

    /// PIN — **a tight buffer is copied exactly**, which is the case a machine
    /// whose decoder needs no alignment produces and the one a stride-aware copy
    /// could quietly get wrong in the other direction.
    #[test]
    fn a_row_with_no_padding_at_all_is_still_the_same_picture() {
        let store: Vec<u8> = (0..16_u8).collect();
        let tight = copy_tight(&store, 8, 2, 2).expect("the rows are copied");
        assert_eq!(tight, store);
    }

    /// PIN — **a stride that is narrower than a row is refused.**
    ///
    /// A buffer that does not describe the picture it claims to, which is the
    /// one shape of this input that would read past the store it was given.
    #[test]
    fn a_stride_narrower_than_a_row_is_refused() {
        let store = vec![0_u8; 64];
        assert!(copy_tight(&store, 4, 2, 2).is_none());
        assert!(copy_tight(&store, 8, 2, 9).is_none(), "past the store");
    }
}
