//! **A video that is playing, decoded by Windows and drawn by this window**
//! (user ruling 2026-08-28, route B slice ①; `docs/DESIGN.md` §7.42).
//!
//! [`super`] answers one question about a file — what does a frame of it look
//! like — and then gives the platform back. This answers the other one: keep
//! answering it, sixty times a second, while a clock runs and a speaker plays,
//! until somebody says stop.
//!
//! # Why the engine and not the browser
//!
//! Route A plays a video by writing an HTML page with a `<video>` in it and
//! handing it to WebView2 (`crate::webview`, `bt_app::player`). It works and it
//! shipped. What it cannot do is any of the four things the scoping note
//! ([`docs/plans/video-preview/scoping-2026-08-26.md`], §5.2) named as the day
//! route B would be needed:
//!
//! 1. a video that is **content inside a document** rather than a page of its
//!    own — a frame in a markdown preview, several clips in one pane, a strip
//!    of stills sampled by scroll position;
//! 2. **one browser engine per pane**, which is the memory a Chromium is;
//! 3. a container **Chromium will not open** — `.mov` is the measured one:
//!    `canPlayType('video/quicktime')` is the empty string and Media Foundation
//!    reads it as an ordinary MPEG-4 file source;
//! 4. **owning the audio and the lifetime** rather than inferring them from a
//!    page.
//!
//! And one more that is not on that list because it was not foreseen: a machine
//! with **no WebView2 runtime at all** plays nothing under route A and plays
//! everything here, because the decoder is part of Windows.
//!
//! `IMFMediaEngine` is the same object model the `<video>` element is — Microsoft
//! says so in as many words ("The `IMFMediaEngine` interface contains methods
//! that map to the HTML5 media elements") — so what is lost by leaving the
//! browser is the *chrome*, not the player. `Play`, `Pause`, `SetCurrentTime`,
//! `SetPlaybackRate`, `SetMuted`, `SetVolume`, `GetDuration` and `IsEnded` are
//! all here, one for one.
//!
//! # Frame server, and where the pixels actually go
//!
//! The engine has three modes. **Rendering mode** hands it an `HWND` or a
//! DirectComposition visual and it draws by itself — which would put a second
//! compositor inside a window that already has one, with its own z-order, its
//! own idea of when a frame is due, and a rectangle this product's layout solver
//! does not own. **Audio-only** is not a video. This module uses the third,
//! **frame-server mode**: the engine decodes and hands over a surface when asked,
//! and every question of *where* and *when* stays on this side.
//!
//! Frame-server mode is what you get when neither `MF_MEDIA_ENGINE_PLAYBACK_HWND`
//! nor `MF_MEDIA_ENGINE_PLAYBACK_VISUAL` is set. Two attributes make it useful:
//! `MF_MEDIA_ENGINE_DXGI_MANAGER`, which is how the engine is told which Direct3D
//! device to decode onto, and `MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT`, which is
//! asked for `DXGI_FORMAT_B8G8R8A8_UNORM` — the same byte order the swapchain
//! this window presents through already uses, so nothing swizzles a pixel
//! anywhere in this file.
//!
//! Then, per frame: `OnVideoStreamTick` says whether there is a new one,
//! `TransferVideoFrame` puts it in a texture this module owns, and the texture is
//! **read back to system memory** and handed to the renderer as bytes.
//!
//! ## Why a read-back and not a shared texture
//!
//! This was the one open question of the slice and it has a structural answer,
//! not a performance one.
//!
//! A shared texture would be `IDXGIResource1::CreateSharedHandle` on the D3D11
//! texture below, `ID3D12Device::OpenSharedHandle` inside wgpu's dx12 backend,
//! and `wgpu_hal::dx12::Device::texture_from_raw` to wrap the result — plus a
//! shared fence on each side, because a keyed mutex is a D3D11 concept that
//! D3D12 does not take. **Every one of those calls is `unsafe`, and the only
//! crate in this workspace allowed to write `unsafe` is this one** (root
//! `Cargo.toml`, `unsafe_code = "deny"`, with `bt-platform` the single
//! exception). The wrapping has to happen where the `wgpu::Device` is, which is
//! `bt-render`; and `bt-render` may not write it. Moving wgpu into `bt-platform`
//! to satisfy that is a much larger edit than this slice — it would make the
//! platform crate depend on the renderer's graphics stack, which is exactly the
//! direction `create_surface`'s note in `bt-render` says the dependency must not
//! run ("`bt-platform` neither depends on wgpu nor knows what a surface is").
//!
//! So the frame comes back through system memory, and the cost of that is
//! measured rather than assumed — see [`Engine::frame_cost`] and the gate
//! `a_frame_arrives_after_play_and_position_advances`, which prints it. It is one
//! `CopyResource` into a staging texture, one `Map`, and one row-wise `memcpy`
//! per frame. **It is also the path that cannot fail on a machine with no
//! graphics driver**, which the release gate's clean VM is: WARP is a Direct3D
//! device like any other and a read-back from it is an ordinary read-back, while
//! a cross-API shared handle on a software adapter is a thing nobody has
//! promised. [`Adapter::Software`] exists so that path is *tested* and not
//! merely hoped for.
//!
//! When slice ② wants the shared texture, nothing here has to be rebuilt: it is
//! a second arm on [`Frame`], and the same [`Engine`] hands it over.
//!
//! # One thread per engine, and no COM anywhere else
//!
//! Media Foundation asks to be spoken to from a multithreaded apartment and
//! every apartment this process had before [`super`] is a single-threaded one on
//! a window's event loop. Rather than reason about whether each of a dozen COM
//! calls marshals correctly out of an STA, **the whole conversation lives on one
//! thread that this module owns**: it joins the process apartment, creates the
//! Direct3D device, creates the engine, answers commands, polls for frames and
//! shuts everything down. Nothing COM-shaped ever crosses back — what crosses is
//! a [`Command`] going in, and a [`Frame`] of plain bytes plus an [`EngineState`]
//! of plain numbers coming out.
//!
//! That is also what makes the notify callback safe. `IMFMediaEngineNotify` is
//! called on a Media Foundation work queue, which is a thread this module has
//! never seen; all it does there is push the event down the same channel the
//! commands use, which wakes the engine thread. **Nothing touches a window, a
//! layout or a renderer from inside a callback**, and there is nothing here that
//! could: the callback's whole world is a `Sender`.
//!
//! # Lifetime
//!
//! One engine per playing video; [`Engine::shutdown`] on the pane or float that
//! closes, and [`Drop`] for everything else, including a panic. Both go to the
//! same place — the thread is told to stop, it calls `IMFMediaEngine::Shutdown`,
//! releases the device and leaves the apartment, and the handle joins it. The
//! platform itself ([`super::media_session`]) is the *process's* and outlives
//! every engine; [`super::shutdown_media_session`] gives it back after the last
//! one has gone, which is what §7.35's exit protocol requires — everything
//! released, and only then the process ends.
//!
//! [`engines_started`] and [`engines_shut_down`] count the two halves so that
//! "no engine outlives the process" is a claim a test can read rather than a
//! promise in a comment.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{RECT, S_OK};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFDXGIDeviceManager, IMFMediaEngine,
    IMFMediaEngineClassFactory, IMFMediaEngineNotify, IMFMediaEngineNotify_Impl,
    MF_MEDIA_ENGINE_CALLBACK, MF_MEDIA_ENGINE_CANPLAY_MAYBE, MF_MEDIA_ENGINE_CANPLAY_PROBABLY,
    MF_MEDIA_ENGINE_DXGI_MANAGER, MF_MEDIA_ENGINE_ERR, MF_MEDIA_ENGINE_ERR_ABORTED,
    MF_MEDIA_ENGINE_ERR_DECODE, MF_MEDIA_ENGINE_ERR_ENCRYPTED, MF_MEDIA_ENGINE_ERR_NETWORK,
    MF_MEDIA_ENGINE_EVENT_ERROR, MF_MEDIA_ENGINE_PRELOAD_AUTOMATIC,
    MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, MFCreateAttributes, MFCreateDXGIDeviceManager,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::{BSTR, Interface, implement};

/// **How often the engine thread asks whether a new picture has arrived, while
/// one is expected.**
///
/// `OnVideoStreamTick` is a cheap question with an unambiguous answer — `S_OK`
/// for a frame and `S_FALSE` for none — so the poll costs a virtual call and a
/// compare, and the interval is chosen against the *display* rather than against
/// the file: four milliseconds is a quarter of a 60 Hz frame, so a picture is
/// never more than a quarter-frame stale by the time the window could draw it,
/// and a 240 Hz panel is still asked often enough to see every one.
///
/// A frame-accurate wait would be `MF_MEDIA_ENGINE_EVENT_FRAMESTEPCOMPLETED`,
/// which the engine only raises in frame-*step* mode — the mode that plays no
/// audio and advances one picture per request. Polling is what the platform's own
/// frame-server sample does.
pub const FRAME_POLL_INTERVAL: Duration = Duration::from_millis(4);

/// **How long the engine thread sleeps when no picture is due** — paused,
/// ended, or not yet loaded.
///
/// It is a wait on the command channel and not a sleep, so a `Play` arriving in
/// the middle of it is answered at once; the number only bounds how stale the
/// [`EngineState`] a caller reads may be while nothing is happening, and nothing
/// happening is the one case where staleness cannot be seen.
pub const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// **How long an engine may take to come into being before not having done so
/// is the fault.**
///
/// Starting Media Foundation, creating a Direct3D device and creating a media
/// engine is a hundred milliseconds on a cold process and forty on a warm one,
/// measured on this machine with `container-probe`. [`Engine::open`] does not
/// wait for any of it — see its own note — so this is not a caller blocking;
/// it is the deadline that turns *an engine that never came up* into an
/// [`EngineError::Unresponsive`] a surface can print, instead of a rectangle
/// that stays black for ever.
///
/// Five seconds, which is two orders of magnitude past the measurement and the
/// same number the window's own watchdog calls a hang.
///
/// It bounds the *creation*, not the *load*: metadata arrives later and
/// asynchronously, which is what [`EngineState::natural_size`] being an
/// [`Option`] means.
pub const OPEN_BUDGET: Duration = Duration::from_secs(5);

/// **Which Direct3D device the frames are decoded onto.**
///
/// [`Adapter::Automatic`] is what the product uses: the hardware adapter, and
/// WARP if there is no usable one — the same order every other graphics stack on
/// Windows tries, and the order the release gate's clean virtual machine
/// exercises the second half of.
///
/// [`Adapter::Software`] forces WARP on a machine that has a perfectly good GPU,
/// which is not a thing a product ever wants and is the only way a test can pin
/// the promise that the frame path does not depend on one. Without it "it works
/// on WARP" would be a claim nobody could check without a second machine.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Adapter {
    #[default]
    Automatic,
    Software,
}

/// **Why an engine could not be made, or could not play what it was given.**
///
/// The five `MF_MEDIA_ENGINE_ERR_*` codes are the HTML5 `MediaError` values with
/// their own names, kept apart rather than collapsed into one silence because a
/// player has somewhere to *say* them: a pane showing a video can print one line
/// under a black rectangle, which is a different product from a pane that shows
/// nothing. [`super::first_frame`] collapses because a hover card has no room for
/// a sentence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineError {
    /// The load was stopped — `MediaError.MEDIA_ERR_ABORTED`.
    Aborted,
    /// The bytes stopped arriving — `MediaError.MEDIA_ERR_NETWORK`.
    Network,
    /// There is a decoder and it could not decode this — `MEDIA_ERR_DECODE`.
    Decode,
    /// There is no decoder, or no source, for this container or codec —
    /// `MEDIA_ERR_SRC_NOT_SUPPORTED`. This is the answer for a `.mkv` and for a
    /// VP9 `.webm` on a machine with no VP9 extension installed.
    Unsupported,
    /// The content is protected and this process has no way to show it.
    Encrypted,
    /// Media Foundation itself would not start — the one failure that is about
    /// the machine rather than about the file.
    NoPlatform,
    /// No Direct3D device could be created, not even WARP.
    NoDevice,
    /// The media engine class factory, the engine, or the device manager
    /// refused. Carries no `HRESULT` on purpose: there is nothing a caller does
    /// differently for one number over another, and a code in a card is a code
    /// a reader has to search for.
    NoEngine,
    /// The engine thread did not answer inside [`OPEN_BUDGET`], or has gone.
    Unresponsive,
}

impl EngineError {
    fn of_code(code: MF_MEDIA_ENGINE_ERR) -> Self {
        match code {
            MF_MEDIA_ENGINE_ERR_ABORTED => Self::Aborted,
            MF_MEDIA_ENGINE_ERR_NETWORK => Self::Network,
            MF_MEDIA_ENGINE_ERR_DECODE => Self::Decode,
            MF_MEDIA_ENGINE_ERR_ENCRYPTED => Self::Encrypted,
            // `MF_MEDIA_ENGINE_ERR_SRC_NOT_SUPPORTED` and anything the platform
            // invents later: the file is not one this machine can open, which is
            // the same sentence for a reader.
            _ => Self::Unsupported,
        }
    }
}

/// **Everything a caller may know about a playing video without touching COM.**
///
/// Plain numbers, copied out under a lock the engine thread holds for the length
/// of one assignment. A caller reads the whole struct at once rather than asking
/// five questions, because five questions answered a microsecond apart are five
/// answers about five different moments — a paused player whose position is
/// still advancing is exactly the sort of contradiction that produces.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineState {
    /// How long the video is, or `None` before the metadata has arrived and for
    /// a source that declares no length.
    pub duration_secs: Option<f64>,
    /// Where the playhead is, in seconds from the start.
    pub position_secs: f64,
    /// Whether the engine is running the clock. **Not** "whether a frame changed
    /// recently": a video paused on its last picture is `false` here and still
    /// has a picture on the glass.
    pub playing: bool,
    /// The video's own pixel dimensions, or `None` before the metadata has
    /// arrived and for a source with no video stream at all.
    pub natural_size: Option<(u32, u32)>,
    /// Whether the playhead has reached the end. An ended video is not playing
    /// and its last frame stands.
    pub ended: bool,
    /// The one thing that went wrong, if one did. Sticky: an engine that has
    /// errored does not un-error.
    pub error: Option<EngineError>,
    pub muted: bool,
    /// `0.0`–`1.0`, the same scale `HTMLMediaElement.volume` uses.
    pub volume: f64,
    /// `1.0` is normal speed.
    pub rate: f64,
    /// Whether the metadata has arrived — the moment [`Self::natural_size`] and
    /// [`Self::duration_secs`] become answerable and a layout can be solved.
    pub ready: bool,
    /// Whether this source has a picture at all. An `.m4a` opened here loads,
    /// plays and never produces a frame, and this is how a caller knows to draw
    /// something other than a black rectangle.
    pub has_video: bool,
    pub has_audio: bool,
}

/// **One decoded picture, in the byte order the swapchain wants.**
///
/// `bgra` is straight (non-premultiplied) BGRA8, row-major, `width * height * 4`
/// bytes and no padding — the staging texture's row pitch is stripped on the way
/// out, because a pitch is a fact about somebody's memory and not about a
/// picture.
///
/// **BGRA and not RGBA**, which is the one difference from
/// [`super::VideoFrame`]. That one is a *picture* joining the window's picture
/// channel, where everything is RGBA; this is a *video frame* on its way to a
/// texture created for it, and the format that texture is created in is chosen
/// here. Asking the engine for BGRA and uploading BGRA means no pixel in a
/// playing video is ever touched by this process's CPU except to be copied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub bgra: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
    /// Counts from one and never repeats for the life of an engine. It is what
    /// lets a renderer skip an upload it has already done, and what makes "a
    /// frame arrived" a thing a test can assert without comparing megabytes.
    pub generation: u64,
}

/// **Where one frame's microseconds went**, for the read-back this slice chose.
///
/// Three spans and a count, and the reason they exist is written in the module
/// note: the shared-texture path was given up for a structural reason, and a
/// structural reason is worth much less if nobody knows what it cost. Slice ②
/// reads these when it decides whether to pay for the sharing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameCost {
    /// `TransferVideoFrame`: the engine composing its decoded picture into this
    /// module's texture, on the GPU.
    pub transfer: Duration,
    /// `CopyResource` into the staging texture plus the `Map` that waits for it
    /// — where a read-back's stall actually is.
    pub readback: Duration,
    /// The row-wise `memcpy` out of the mapped rows into a `Vec`.
    pub copy: Duration,
    /// How many frames the three spans above are the *last* of.
    pub frames: u64,
}

impl FrameCost {
    /// The three spans added up — the length of one frame's whole crossing.
    #[must_use]
    pub fn total(self) -> Duration {
        self.transfer + self.readback + self.copy
    }
}

/// **A video that is loaded, and a thread that is looking after it.**
///
/// The handle is ordinary Rust: no COM interface is reachable through it, so it
/// may be held by a pane, moved between them, and dropped on any thread. Every
/// method is a message to the engine thread except [`Self::state`] and
/// [`Self::frame`], which read what that thread last published.
///
/// **Commands are fire-and-forget and that is deliberate.** `Play` on an engine
/// whose thread has already stopped is not an error a caller can do anything
/// about — the state will say so on the next read — and a `Result` on every verb
/// would put error handling on six call sites that would all discard it.
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
    /// **Open `path` on whichever Direct3D device this machine offers.**
    ///
    /// Returns when the engine has been *asked for*, not when it exists and not
    /// when the video has loaded. A caller that needs the size or the length
    /// watches [`EngineState::ready`]; a caller that needs to know whether the
    /// file can be played at all watches [`EngineState::error`], which is where
    /// a refusal arrives — including a refusal to build the engine in the first
    /// place, and including [`EngineError::Unresponsive`] for one that has not
    /// come up inside [`OPEN_BUDGET`].
    ///
    /// # Where it may be called from
    ///
    /// Any thread, including a window's, and this function **waits for
    /// nothing**. It allocates a channel and starts a thread; `MFStartup`, the
    /// Direct3D device, the media engine and `SetSource` are all on the far side
    /// of that thread's first instruction.
    ///
    /// It was not always so, and the difference is the whole of §7.42's rule
    /// about the drawing thread. Until 2026-08-28 this waited for the far side
    /// to report that the engine existed — a hundred milliseconds on the first
    /// video of a process and forty on every one after, paid by whichever thread
    /// pressed play, with a five-second worst case behind it. A window does not
    /// have a hundred milliseconds to give a button press.
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        Self::open_on(path, Adapter::Automatic)
    }

    /// The same, with the choice of device forced — see [`Adapter`].
    pub fn open_on(path: &Path, adapter: Adapter) -> Result<Self, EngineError> {
        let shared = Arc::new(Shared::default());
        let (commands, inbox) = mpsc::channel();
        let source = path.to_path_buf();
        // UTF-16 across the thread boundary and a `BSTR` built on the far side:
        // a `BSTR` is a raw pointer and therefore not `Send`, and a lossy
        // `String` would rename a file whose name Windows lets be unpaired.
        let url: Vec<u16> = {
            use std::os::windows::ffi::OsStrExt as _;
            path.as_os_str().encode_wide().collect()
        };
        let thread = {
            let shared = Arc::clone(&shared);
            let commands = commands.clone();
            std::thread::Builder::new()
                .name("folio-video-engine".to_owned())
                .spawn(move || {
                    run(&BSTR::from_wide(&url), adapter, &shared, &commands, &inbox);
                })
                // The one thing that can still fail here, and it fails without
                // having started anything: a process out of thread handles.
                .map_err(|_| EngineError::Unresponsive)?
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

    /// Everything knowable about this engine at this instant. See [`EngineState`].
    ///
    /// **[`OPEN_BUDGET`] is spent here**, and it is the only place it is spent.
    /// [`Self::open`] waits for nothing, so nobody is left holding a timer when
    /// an engine fails to come into being — a `CoCreateInstance` that never
    /// returns, a device stuck in a reset. This reading is what ends that: past
    /// the budget with nothing built and nothing said, the answer is
    /// [`EngineError::Unresponsive`], which is a sentence a surface already
    /// knows how to print. Stable once it is true, so it is as sticky as the
    /// errors the engine thread publishes itself.
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
                error: Some(EngineError::Unresponsive),
                ..state
            };
        }
        state
    }

    /// **The newest picture, if it is newer than the last one this handle
    /// returned**, and `None` otherwise.
    ///
    /// `&mut self` is the whole contract: "newer than the last one" is a fact
    /// about *this handle*, so one caller draining frames cannot make another
    /// caller's `frame()` answer nothing. It also means a renderer can call this
    /// every frame of its own loop, get `Some` at the video's rate rather than at
    /// its own, and upload exactly as many textures as there were pictures.
    ///
    /// Frames are **not queued**. The engine thread keeps the most recent one
    /// and drops the one before it, because a video is a clock: a caller that
    /// fell behind wants the picture that is due now, not the four it missed.
    /// [`Frame::generation`] is what says how many were skipped.
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

    /// Which device the frames are being decoded onto, once it is known.
    /// `Some(Adapter::Software)` on a machine with no graphics driver.
    #[must_use]
    pub fn adapter_in_use(&self) -> Option<Adapter> {
        match self.shared.adapter.load(Ordering::Relaxed) {
            1 => Some(Adapter::Automatic),
            2 => Some(Adapter::Software),
            _ => None,
        }
    }

    pub fn play(&self) {
        let _ = self.commands.send(Command::Play);
    }

    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }

    /// Move the playhead. Out-of-range values are the engine's business to
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

    /// **Stop, release the engine and join its thread**, now rather than at drop.
    ///
    /// The verb a pane calls when it closes or is handed a different file. It is
    /// idempotent and [`Drop`] calls it, so a caller that forgets is not a caller
    /// that leaks — see [`engines_shut_down`].
    pub fn shutdown(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
    generation: AtomicU64,
    /// `0` unknown, `1` hardware, `2` WARP — an atomic rather than a lock
    /// because it is written once.
    adapter: AtomicU32,
    /// **Whether the engine thread got as far as having an engine.** Set once,
    /// by [`Machinery::build`]'s caller, immediately before the pump starts.
    /// [`Engine::state`] reads it to decide whether [`OPEN_BUDGET`] has been
    /// missed — a question that only has an answer while this is `false`.
    built: AtomicBool,
}

enum Command {
    Play,
    Pause,
    Seek(f64),
    Rate(f64),
    Muted(bool),
    Volume(f64),
    /// One `IMFMediaEngineNotify::EventNotify`, forwarded off the work queue
    /// thread it arrived on. The engine thread reads the engine, not this
    /// payload, for everything except the error code — an event says *when* to
    /// look, and the engine says what is true.
    Event(u32),
    Shutdown,
}

/// How many media engines this process has created, and how many it has shut
/// down. Equal at every moment no engine is alive, which is what the structural
/// gate reads: an [`Engine`] that is dropped, forgotten, or unwound past by a
/// panic goes through the same [`Engine::shutdown`].
#[must_use]
pub fn engines_started() -> u64 {
    ENGINES_STARTED.load(Ordering::Relaxed)
}

/// The other half of [`engines_started`].
#[must_use]
pub fn engines_shut_down() -> u64 {
    ENGINES_SHUT_DOWN.load(Ordering::Relaxed)
}

/// How many engines are alive right now. **Zero is the only value this may have
/// when the process leaves**, which is what [`super::shutdown_media_session`]
/// asserts on a debug build.
#[must_use]
pub fn engines_outstanding() -> u64 {
    engines_started().saturating_sub(engines_shut_down())
}

static ENGINES_STARTED: AtomicU64 = AtomicU64::new(0);
static ENGINES_SHUT_DOWN: AtomicU64 = AtomicU64::new(0);

/// **The whole of the engine thread**: an apartment, a device, an engine, a
/// loop, and the giving back of all four in the reverse order.
/// **What this machine says about a media type, before a file of it is opened.**
///
/// The three answers `HTMLMediaElement.canPlayType` gives, and they are the same
/// three because [`Engine`] is the same object model — §7.16 measured the
/// browser's answers with exactly this question and got the empty string for
/// `video/quicktime`, which is [`Self::No`].
///
/// **It is an opinion and not a promise**, in both directions. `Maybe` is the
/// platform saying it has a source and a decoder for the container and codec
/// named, not that a particular file is well formed; and a type it answers `No`
/// to is one no file will play, which is the useful half. What it is *for* is a
/// matrix that can be written down for a given machine — see
/// [`can_play_types`] — rather than a list of formats guessed from
/// documentation that was last updated in 2018.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CanPlay {
    No,
    Maybe,
    Probably,
}

/// **Ask this machine about a list of media types**, in one engine and one
/// apartment.
///
/// One engine for the whole list because the answer is a property of the
/// installed decoders and not of any engine, and because building one costs a
/// Direct3D device. An engine that could not be built answers [`CanPlay::No`]
/// for everything, which is the honest reading: a machine where this fails plays
/// none of them here.
///
/// # Where it may be called from
///
/// Any thread. It runs its own and joins it, so it blocks for as long as
/// creating an engine takes — which makes it a start-up or a diagnostic
/// question, not a per-hover one.
#[must_use]
pub fn can_play_types(types: &[&str]) -> Vec<CanPlay> {
    let none = || vec![CanPlay::No; types.len()];
    if !super::media_session() {
        return none();
    }
    // Owned `String`s across the boundary and `BSTR`s built on the far side, for
    // the reason [`Engine::open`] states: a `BSTR` is a raw pointer and is not
    // `Send`.
    let asked: Vec<String> = types.iter().map(|kind| (*kind).to_owned()).collect();
    let Ok(thread) = std::thread::Builder::new()
        .name("folio-video-canplay".to_owned())
        .spawn(move || {
            // SAFETY: an MTA entry on this thread, released below on every path.
            let apartment = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if apartment.is_err() {
                return Vec::new();
            }
            let answers = Machinery::build(
                None,
                Adapter::Automatic,
                &Arc::new(Shared::default()),
                &mpsc::channel().0,
            )
            .map(|mut machinery| {
                let answers = asked
                    .iter()
                    // SAFETY: a COM method on this thread's own engine.
                    .map(|kind| {
                        match unsafe { machinery.engine.CanPlayType(&BSTR::from(kind.as_str())) } {
                            Ok(MF_MEDIA_ENGINE_CANPLAY_PROBABLY) => CanPlay::Probably,
                            Ok(MF_MEDIA_ENGINE_CANPLAY_MAYBE) => CanPlay::Maybe,
                            _ => CanPlay::No,
                        }
                    })
                    .collect::<Vec<_>>();
                machinery.stop();
                ENGINES_SHUT_DOWN.fetch_add(1, Ordering::Relaxed);
                answers
            })
            .unwrap_or_default();
            // SAFETY: paired with the `CoInitializeEx` above, after every
            // interface this thread made has been dropped.
            unsafe { CoUninitialize() };
            answers
        })
    else {
        return none();
    };
    let answers = thread.join().unwrap_or_default();
    if answers.len() == types.len() {
        answers
    } else {
        none()
    }
}

/// **Everything an engine costs, on the engine's own thread.**
///
/// Starting Media Foundation, joining the apartment, building the machinery and
/// running the pump — in that order, and none of it on the caller's thread.
/// `media_session` in particular is here rather than in [`Engine::open_on`]: it
/// is a `OnceLock` around `MFStartup`, so exactly one video open in the life of
/// a process pays for it, and the thread that pays must not be one with a window
/// on it.
///
/// Nothing is reported back through a channel, because nobody is waiting on one.
/// A failure at any point is written into the shared [`EngineState`] as its
/// `error`, which is the same slot a codec refusing a file a minute in writes
/// to and the same slot every surface already reads.
fn run(
    url: &BSTR,
    adapter: Adapter,
    shared: &Arc<Shared>,
    commands: &mpsc::Sender<Command>,
    inbox: &mpsc::Receiver<Command>,
) {
    if !super::media_session() {
        publish_failure(shared, EngineError::NoPlatform);
        return;
    }
    // SAFETY: an MTA entry on this thread, released on every path out of this
    // function. The process apartment is already standing (`media_session`
    // returned true just above), so this is a reference count.
    let apartment = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if apartment.is_err() {
        publish_failure(shared, EngineError::NoPlatform);
        return;
    }
    match Machinery::build(Some(url), adapter, shared, commands) {
        Ok(mut machinery) => {
            // Before the pump and after the engine: this is what stops
            // `Engine::state` charging a working engine with having missed
            // `OPEN_BUDGET`.
            shared.built.store(true, Ordering::Release);
            machinery.pump(shared, inbox);
            machinery.stop();
            ENGINES_SHUT_DOWN.fetch_add(1, Ordering::Relaxed);
        }
        Err(error) => publish_failure(shared, error),
    }
    // SAFETY: paired with the `CoInitializeEx` above, on its thread, after every
    // interface this thread made has been dropped.
    unsafe { CoUninitialize() };
}

/// An engine that never came into being, said in the one place a caller looks.
fn publish_failure(shared: &Arc<Shared>, error: EngineError) {
    let mut state = shared.state.lock().unwrap_or_else(|held| held.into_inner());
    if state.error.is_none() {
        state.error = Some(error);
    }
}

/// Everything the engine thread owns. It never leaves that thread — the type is
/// deliberately not `Send`, because the COM interfaces in it are not.
struct Machinery {
    engine: IMFMediaEngine,
    /// Held for its lifetime and not read: the engine holds a reference of its
    /// own, and this is here so that dropping the machinery drops the callback
    /// after the engine that could call it.
    _notify: IMFMediaEngineNotify,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// The engine decodes into this; it is created at the video's own size the
    /// first time the size is known.
    target: Option<(ID3D11Texture2D, ID3D11Texture2D, u32, u32)>,
    generation: u64,
    error: Option<EngineError>,
}

impl Machinery {
    fn build(
        url: Option<&BSTR>,
        adapter: Adapter,
        shared: &Arc<Shared>,
        commands: &mpsc::Sender<Command>,
    ) -> Result<Self, EngineError> {
        let (device, context, software) = create_device(adapter)?;
        shared
            .adapter
            .store(if software { 2 } else { 1 }, Ordering::Relaxed);
        // SAFETY: every call below is a COM method on an interface this function
        // created and holds, on the thread that created it, inside the apartment
        // `run` entered.
        unsafe {
            // **Multithread protection, and it is not optional.** Media
            // Foundation's decoder runs on its own work queue and reaches this
            // device through the DXGI manager; this thread reaches the same
            // device through `context` to copy the frame out. A D3D11 immediate
            // context is not thread-safe, and this is the platform's own way of
            // making one so — the alternative is `IMFDXGIDeviceManager::LockDevice`
            // around every call, which is the same lock with more places to
            // forget it.
            if let Ok(multithread) = device.cast::<ID3D11Multithread>() {
                let _ = multithread.SetMultithreadProtected(true);
            }
            let mut reset_token = 0_u32;
            let mut manager: Option<IMFDXGIDeviceManager> = None;
            MFCreateDXGIDeviceManager(&mut reset_token, &mut manager)
                .map_err(|_| EngineError::NoEngine)?;
            let manager = manager.ok_or(EngineError::NoEngine)?;
            manager
                .ResetDevice(&device, reset_token)
                .map_err(|_| EngineError::NoEngine)?;

            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(|_| EngineError::NoEngine)?;
            let notify: IMFMediaEngineNotify = Notify {
                commands: Mutex::new(commands.clone()),
            }
            .into();
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 4).map_err(|_| EngineError::NoEngine)?;
            let attributes = attributes.ok_or(EngineError::NoEngine)?;
            attributes
                .SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)
                .map_err(|_| EngineError::NoEngine)?;
            attributes
                .SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, &manager)
                .map_err(|_| EngineError::NoEngine)?;
            // Frame-server mode is what you get by *not* naming a window or a
            // visual; this only says what the frames it serves should look like.
            // The same eight bytes per pixel the swapchain wants, so the whole
            // path from decoder to glass never reorders a channel.
            attributes
                .SetUINT32(
                    &MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT,
                    DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32,
                )
                .map_err(|_| EngineError::NoEngine)?;
            let engine = factory
                .CreateInstance(0, &attributes)
                .map_err(|_| EngineError::NoEngine)?;
            // Counted where the engine actually comes into being, so that every
            // path that makes one — a playback, a `can_play_types` probe — is on
            // the same ledger as the `Shutdown` that ends it.
            ENGINES_STARTED.fetch_add(1, Ordering::Relaxed);
            let _ = engine.SetPreload(MF_MEDIA_ENGINE_PRELOAD_AUTOMATIC);
            // **Not autoplay.** The reader presses play; an engine that started
            // on its own would make "the pane is showing a still" a state the
            // product could not be in.
            let _ = engine.SetAutoPlay(false);
            // **`None` is an engine with nothing to play**, which is what
            // [`can_play_types`] wants: a source would make it load a file in
            // order to answer a question about a type.
            if let Some(url) = url {
                engine.SetSource(url).map_err(|_| EngineError::NoEngine)?;
            }
            Ok(Self {
                engine,
                _notify: notify,
                device,
                context,
                target: None,
                generation: 0,
                error: None,
            })
        }
    }

    /// The loop: answer commands, publish state, and take a picture when there
    /// is one.
    fn pump(&mut self, shared: &Arc<Shared>, inbox: &mpsc::Receiver<Command>) {
        loop {
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
                Ok(Command::Shutdown) => return,
                Ok(command) => {
                    self.apply(command);
                    // Drain whatever else is already waiting before spending a
                    // poll on it: a burst of events during a load is one look at
                    // the engine, not eight.
                    while let Ok(next) = inbox.try_recv() {
                        if matches!(next, Command::Shutdown) {
                            return;
                        }
                        self.apply(next);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                // Every handle has gone without saying so — a panic between the
                // `Engine` being made and being dropped. Same ending.
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            self.take_frame(shared);
        }
    }

    fn apply(&mut self, command: Command) {
        // SAFETY: COM methods on this thread's own engine, inside its apartment.
        unsafe {
            match command {
                Command::Play => {
                    let _ = self.engine.Play();
                }
                Command::Pause => {
                    let _ = self.engine.Pause();
                }
                Command::Seek(secs) => {
                    let _ = self.engine.SetCurrentTime(secs.max(0.0));
                }
                Command::Rate(rate) => {
                    let _ = self.engine.SetPlaybackRate(rate);
                }
                Command::Muted(muted) => {
                    let _ = self.engine.SetMuted(muted);
                }
                Command::Volume(volume) => {
                    let _ = self.engine.SetVolume(volume.clamp(0.0, 1.0));
                }
                Command::Event(event) => self.observe(event),
                Command::Shutdown => {}
            }
        }
    }

    /// What one notification means here. Almost nothing: the state is read off
    /// the engine every turn of the loop anyway, so an event's only job is to
    /// wake that loop up. The exception is the error, whose code lives on an
    /// `IMFMediaError` that is gone by the time anybody asks again.
    fn observe(&mut self, event: u32) {
        if event == MF_MEDIA_ENGINE_EVENT_ERROR.0 as u32 {
            // SAFETY: on the engine's own thread and apartment.
            let code = unsafe {
                self.engine
                    .GetError()
                    .ok()
                    .map(|error| error.GetErrorCode())
            };
            self.error = Some(code.map_or(EngineError::Decode, |code| {
                EngineError::of_code(MF_MEDIA_ENGINE_ERR(i32::from(code)))
            }));
        }
        // Every other event — `LOADEDMETADATA`, `CANPLAY`, `ENDED`,
        // `TIMEUPDATE`, `VOLUMECHANGE` — reaches the loop as a *wake* and is
        // then read back off the engine, which is what keeps one authority for
        // every number on [`EngineState`]. An event carries no payload this
        // module trusts over the object that raised it.
    }

    fn publish_state(&mut self, shared: &Arc<Shared>) -> EngineState {
        // SAFETY: COM methods on this thread's own engine, inside its apartment.
        let state = unsafe {
            let mut width = 0_u32;
            let mut height = 0_u32;
            let sized = self
                .engine
                .GetNativeVideoSize(Some(&mut width), Some(&mut height))
                .is_ok()
                && width > 0
                && height > 0;
            let duration = self.engine.GetDuration();
            EngineState {
                duration_secs: duration
                    .is_finite()
                    .then_some(duration)
                    .filter(|d| *d > 0.0),
                position_secs: self.engine.GetCurrentTime(),
                playing: !self.engine.IsPaused().as_bool() && !self.engine.IsEnded().as_bool(),
                natural_size: sized.then_some((width, height)),
                ended: self.engine.IsEnded().as_bool(),
                error: self.error,
                muted: self.engine.GetMuted().as_bool(),
                volume: self.engine.GetVolume(),
                rate: self.engine.GetPlaybackRate(),
                // **The size, not an event.** `HAVE_METADATA` is `readyState`
                // ≥ 1, and the one thing every caller wants out of it is the
                // size — so "ready" is spelled as the size being answerable,
                // which cannot be true and useless.
                ready: sized || self.engine.HasAudio().as_bool(),
                has_video: self.engine.HasVideo().as_bool(),
                has_audio: self.engine.HasAudio().as_bool(),
            }
        };
        *shared.state.lock().unwrap_or_else(|held| held.into_inner()) = state;
        state
    }

    /// Ask whether there is a new picture and, if there is, bring it back.
    fn take_frame(&mut self, shared: &Arc<Shared>) {
        // SAFETY: the vtable is called directly rather than through the
        // generated wrapper because the wrapper cannot express this call's
        // answer: `OnVideoStreamTick` returns `S_OK` for a new frame and
        // `S_FALSE` for none, and `HRESULT::and_then` treats both as success.
        // Everything else here is a COM method on this thread's own interfaces.
        unsafe {
            let mut presentation = 0_i64;
            let ticked = (Interface::vtable(&self.engine).OnVideoStreamTick)(
                Interface::as_raw(&self.engine),
                &mut presentation,
            );
            if ticked != S_OK {
                return;
            }
            let mut width = 0_u32;
            let mut height = 0_u32;
            if self
                .engine
                .GetNativeVideoSize(Some(&mut width), Some(&mut height))
                .is_err()
                || width == 0
                || height == 0
            {
                return;
            }
            if !matches!(self.target, Some((_, _, w, h)) if w == width && h == height) {
                self.target = self.create_target(width, height);
            }
            let Some((target, staging, _, _)) = self.target.as_ref() else {
                return;
            };
            let mut cost = FrameCost::default();
            let started = Instant::now();
            let destination = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            if self
                .engine
                .TransferVideoFrame(target, None, &destination, None)
                .is_err()
            {
                return;
            }
            cost.transfer = started.elapsed();

            let started = Instant::now();
            self.context.CopyResource(staging, target);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if self
                .context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .is_err()
            {
                return;
            }
            cost.readback = started.elapsed();

            let started = Instant::now();
            let row_bytes = width as usize * 4;
            let pitch = mapped.RowPitch as usize;
            let bgra = (!mapped.pData.is_null() && pitch >= row_bytes).then(|| {
                let mut out = vec![0_u8; row_bytes * height as usize];
                for row in 0..height as usize {
                    // SAFETY: the mapped subresource is `height` rows of at
                    // least `row_bytes` bytes, `pitch` apart, for as long as the
                    // map is held — which is until the `Unmap` below.
                    let source = std::slice::from_raw_parts(
                        mapped.pData.cast::<u8>().add(row * pitch),
                        row_bytes,
                    );
                    out[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(source);
                }
                out
            });
            self.context.Unmap(staging, 0);
            cost.copy = started.elapsed();

            let Some(bgra) = bgra else {
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

    /// The pair of textures a frame crosses: one the engine may render into, and
    /// one the CPU may read.
    ///
    /// Two and not one because a Direct3D texture cannot be both a render target
    /// and CPU-readable — the staging copy is the crossing, and it is the price
    /// the module note weighs.
    fn create_target(
        &self,
        width: u32,
        height: u32,
    ) -> Option<(ID3D11Texture2D, ID3D11Texture2D, u32, u32)> {
        let common = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..Default::default()
        };
        // SAFETY: two descriptors this function filled in, on this thread's own
        // device.
        unsafe {
            let mut target = None;
            self.device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0)
                            as u32,
                        ..common
                    },
                    None,
                    Some(&mut target),
                )
                .ok()?;
            let mut staging = None;
            self.device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Usage: D3D11_USAGE_STAGING,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        ..common
                    },
                    None,
                    Some(&mut staging),
                )
                .ok()?;
            Some((target?, staging?, width, height))
        }
    }

    /// `IMFMediaEngine::Shutdown`, which is the one call that has to happen
    /// before the interfaces are dropped: it stops the decoder, releases the
    /// device it was given and unhooks the callback. A released-but-not-shut-down
    /// engine keeps a work queue alive.
    fn stop(&mut self) {
        // SAFETY: on the engine's own thread and apartment, once.
        unsafe {
            let _ = self.engine.Shutdown();
            let _ = &self.context;
        }
    }
}

/// **The one object Media Foundation calls back into**, and the whole of what it
/// may do.
///
/// It arrives on a work queue thread — never a window's, never the engine
/// thread's — so the only safe thing to do with it is to say that something
/// happened somewhere else. That is exactly what it does: one `send`. Everything
/// a reader could want to know is read off the engine afterwards, on the thread
/// that owns it.
#[implement(IMFMediaEngineNotify)]
struct Notify {
    /// A `Sender` is `Send` and not `Sync`, and a COM object may be called from
    /// two work queue threads at once; the lock is what makes the second one
    /// wait rather than a data race.
    commands: Mutex<mpsc::Sender<Command>>,
}

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, _param1: usize, _param2: u32) -> windows::core::Result<()> {
        if let Ok(commands) = self.commands.lock() {
            // A closed channel is an engine whose thread has already stopped,
            // which is an ordinary ending and not an error to report to the
            // platform: answering anything but `Ok` here makes the engine treat
            // its own callback as broken.
            let _ = commands.send(Command::Event(event));
        }
        Ok(())
    }
}

/// **A Direct3D 11 device for the decoder to decode onto, hardware if there is
/// one.**
///
/// `VIDEO_SUPPORT` is what lets the engine bind a hardware decoder to it;
/// `BGRA_SUPPORT` is what lets it produce the format asked for above. Neither
/// flag is available on every machine, so a device that refuses them is asked
/// for again without them — a decode that falls back to software inside the
/// platform is still a decode, and a refusal here would be a machine that plays
/// nothing.
///
/// **WARP is a real answer and not a failure.** The release gate's clean virtual
/// machine has no graphics driver, and every frame in this module reaches system
/// memory anyway; a software rasteriser is slower at the `TransferVideoFrame`
/// and identical everywhere else.
fn create_device(
    adapter: Adapter,
) -> Result<(ID3D11Device, ID3D11DeviceContext, bool), EngineError> {
    let levels = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_0,
    ];
    let mut attempts: Vec<(D3D_DRIVER_TYPE, u32)> = Vec::new();
    let full = D3D11_CREATE_DEVICE_BGRA_SUPPORT.0 | D3D11_CREATE_DEVICE_VIDEO_SUPPORT.0;
    if adapter == Adapter::Automatic {
        attempts.push((D3D_DRIVER_TYPE_HARDWARE, full));
        attempts.push((D3D_DRIVER_TYPE_HARDWARE, D3D11_CREATE_DEVICE_BGRA_SUPPORT.0));
    }
    attempts.push((D3D_DRIVER_TYPE_WARP, full));
    attempts.push((D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_BGRA_SUPPORT.0));
    for (driver, flags) in attempts {
        let mut device = None;
        let mut context = None;
        // SAFETY: a platform entry with no interface handed in and two handed
        // back, both of which are checked before use.
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                windows::Win32::Foundation::HMODULE::default(),
                windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_FLAG(flags),
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        if created.is_ok()
            && let (Some(device), Some(context)) = (device, context)
        {
            return Ok((device, context, driver == D3D_DRIVER_TYPE_WARP));
        }
    }
    Err(EngineError::NoDevice)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-assets")
            .join(name)
    }

    /// **Hold the process still while an engine is counted.**
    ///
    /// [`engines_outstanding`] is a fact about a *process*, and the tests in
    /// this module all run in one. Every test here that creates an engine takes
    /// this, so that the ledger gate below can read the counters as an absolute
    /// rather than as a number two other threads are also moving — which is
    /// exactly what it did on the first full-workspace run, where three arms
    /// that are each correct in isolation added up to a red.
    ///
    /// It is not a fix to the invariant and must not be mistaken for one: the
    /// engines are perfectly safe to open concurrently and nothing in the
    /// product serialises them. What cannot be done concurrently is *reading a
    /// global counter and concluding something from the number*.
    fn ledger_gate() -> std::sync::MutexGuard<'static, ()> {
        static GATE: Mutex<()> = Mutex::new(());
        GATE.lock().unwrap_or_else(|held| held.into_inner())
    }

    /// **Wait for the ledger to reach `target`**, and answer where it actually
    /// got to.
    ///
    /// Since [`Engine::open`] stopped waiting for the engine to be built, "an
    /// engine exists" is a thing that becomes true shortly *after* the open
    /// returns rather than before it. A test that reads the counter on the next
    /// instruction is reading a race, so it reads a short wait instead — the
    /// property being pinned is that the number comes up and goes back down, not
    /// that it does so before the caller's next line.
    ///
    /// Under the [`ledger_gate`], so nothing else is moving this number.
    fn engines_settling_to(target: u64) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let now = engines_outstanding();
            if now == target || Instant::now() >= deadline {
                return now;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// RED — **opening a video never blocks the thread that asked** (the freeze
    /// of 2026-08-28; §7.42's rule about the drawing thread).
    ///
    /// `Engine::open` is called from a window's own thread — it is what a press
    /// of ▶ does — and until this slice it waited there for the far side to
    /// build a Direct3D device and a media engine and report back. Measured with
    /// `container-probe`: **112 ms for the first video of a process and 36–45 ms
    /// for every one after**, with a five-second worst case behind it. A window
    /// does not have that to give a button press.
    ///
    /// The bound is stated over **eight opens in a row** rather than over one,
    /// and that is what makes it a gate rather than a coin toss. A blocking open
    /// costs its forty milliseconds *every time*, warm or cold, so eight of them
    /// is a third of a second — five times this budget, on the fastest path the
    /// old code had. An open that waits for nothing costs a channel and a
    /// `spawn`, and eight of those do not add up to a frame.
    ///
    /// Every engine is shut down before the assertion, so this test leaves the
    /// ledger where it found it — and the shutdowns are deliberately *outside*
    /// the measured span, since joining a thread is a wait and this is a test
    /// about not waiting.
    ///
    /// MUTATION: put the `opened.recv_timeout(OPEN_BUDGET)` back into
    /// `Engine::open_on` and this goes red by roughly an order of magnitude.
    #[test]
    fn opening_a_video_never_blocks_the_window_thread() {
        let _ledger = ledger_gate();
        /// Eight opens, and the budget for all eight of them together.
        const OPENS: usize = 8;
        const BUDGET: Duration = Duration::from_millis(60);
        let path = fixture("folio-video-test.mp4");
        let began = Instant::now();
        let mut engines = Vec::with_capacity(OPENS);
        for _ in 0..OPENS {
            engines.push(Engine::open(&path).expect("an engine is asked for"));
        }
        let asking = began.elapsed();
        for mut engine in engines {
            engine.shutdown();
        }
        assert!(
            asking < BUDGET,
            "{OPENS} opens took {asking:?}, which is a window thread waiting for a decoder"
        );
    }

    /// RED — **an engine reports the duration and the size of a file it
    /// opened** (user ruling 2026-08-28; `docs/DESIGN.md` §7.42).
    ///
    /// The first of the four gates, and the one that says the whole creation
    /// chain is wired: the apartment, the Direct3D device, the DXGI device
    /// manager, the class factory, the callback attribute, the output format and
    /// `SetSource`. Every one of those failing ends in an engine that never
    /// answers a size, so a size is what is asserted.
    ///
    /// It is the same five-second, 160×120 fixture [`super::super`]'s own gate
    /// decodes, so a disagreement between the two is a disagreement about the
    /// platform and not about the file.
    ///
    /// RED GATE: drop the `MF_MEDIA_ENGINE_CALLBACK` attribute and the engine
    /// loads nothing and this never becomes ready; ask for a video output format
    /// the engine will not serve and the same.
    #[test]
    fn an_engine_reports_the_duration_and_size_of_a_file_it_opened() {
        let _ledger = ledger_gate();
        let mut engine = Engine::open(&fixture("folio-video-test.mp4")).expect("an engine opens");
        assert!(
            engine.wait_for_metadata(Duration::from_secs(10)),
            "metadata: {:?}",
            engine.state()
        );
        let state = engine.state();
        assert_eq!(
            state.natural_size,
            Some((160, 120)),
            "the engine reports the video's own size"
        );
        let duration = state
            .duration_secs
            .expect("the container declares a length");
        assert!(
            (4.8..=5.2).contains(&duration),
            "a five-second fixture: {duration}s"
        );
        assert!(state.has_video, "{state:?}");
        assert!(!state.playing, "an engine does not start on its own");
        assert_eq!(state.error, None, "{state:?}");
        engine.shutdown();
    }

    /// RED — **a frame arrives after `play`, and the position advances**
    /// (user ruling 2026-08-28; §7.42).
    ///
    /// The difference between this slice and slice ① in one assertion: not "a
    /// picture can be decoded" but "pictures keep coming and time is running".
    /// Both halves are needed — an engine that produced one frame and stopped
    /// would pass a frame-count assertion, and one that ran the clock over a
    /// frozen picture would pass a position assertion.
    ///
    /// The picture is also checked for ink, for [`super::super::SEEK_FRACTION`]'s
    /// reason: this fixture opens on black, so a build that served the decoder's
    /// first buffer for ever would satisfy every other line here.
    ///
    /// The cost line is printed and nothing is concluded from it — it is the
    /// measurement the module note's read-back decision is weighed against.
    ///
    /// **The waits are generous on purpose.** What is being asserted is that
    /// frames arrive at all; how many arrive in a second is a fact about the
    /// machine the test is running on, and this suite runs two dozen other tests
    /// beside it.
    #[test]
    fn a_frame_arrives_after_play_and_position_advances() {
        let _ledger = ledger_gate();
        let mut engine = Engine::open(&fixture("folio-video-test.mp4")).expect("an engine opens");
        assert!(engine.wait_for_metadata(Duration::from_secs(10)));
        engine.set_muted(true);
        engine.play();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut frames = Vec::new();
        let mut advanced = false;
        while Instant::now() < deadline && (frames.len() < 3 || !advanced) {
            if let Some(frame) = engine.frame() {
                frames.push(frame);
            }
            if engine.state().position_secs > 0.05 {
                advanced = true;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        let cost = engine.frame_cost();
        eprintln!(
            "VIDEO_ENGINE_FRAME adapter={:?} frames={} transfer={:?} readback={:?} copy={:?} \
             total={:?}",
            engine.adapter_in_use(),
            cost.frames,
            cost.transfer,
            cost.readback,
            cost.copy,
            cost.total()
        );
        assert!(
            frames.len() >= 3,
            "three pictures in fifteen seconds: {} ({:?})",
            frames.len(),
            engine.state()
        );
        assert!(advanced, "the clock ran: {:?}", engine.state());
        // Every frame is its own generation, and none of them repeats.
        for pair in frames.windows(2) {
            assert!(
                pair[1].generation > pair[0].generation,
                "a generation never goes backwards"
            );
        }
        let last = frames.last().expect("three pictures");
        assert_eq!((last.width, last.height), (160, 120));
        assert_eq!(
            last.bgra.len(),
            160 * 120 * 4,
            "no padding survives the copy"
        );
        let lit = frames.iter().any(|frame| {
            frame
                .bgra
                .chunks_exact(4)
                .any(|pixel| pixel[..3] != [0, 0, 0])
        });
        assert!(lit, "the pictures are not all the opening black frame");
        engine.shutdown();
    }

    /// RED — **the `.mov` this window could never play in a browser plays here**
    /// (user ruling 2026-08-28, trigger ③; §7.42).
    ///
    /// The whole of trigger ③ in one test. `canPlayType('video/quicktime')` in
    /// the engine on the preview seat is the empty string — measured, §7.16 —
    /// and Media Foundation reads the same file as an ordinary MPEG-4 file
    /// source. §7.23 (f) had to write a class with two columns because of that
    /// mismatch; this is the assertion that closes it from the other side.
    ///
    /// RED GATE: if this ever goes red, route B has lost its third reason for
    /// existing and the two-column class becomes correct again — which is the
    /// only circumstance in which it is.
    #[test]
    fn a_quicktime_file_plays_where_the_browser_would_not_open_it() {
        let _ledger = ledger_gate();
        let mut engine = Engine::open(&fixture("folio-video-test.mov")).expect("an engine opens");
        assert!(
            engine.wait_for_metadata(Duration::from_secs(10)),
            "metadata: {:?}",
            engine.state()
        );
        assert_eq!(engine.state().natural_size, Some((160, 120)));
        engine.set_muted(true);
        engine.play();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut frame = None;
        while Instant::now() < deadline && frame.is_none() {
            frame = engine.frame();
            std::thread::sleep(Duration::from_millis(4));
        }
        let frame = frame.expect("a quicktime file gives up frames to this decoder");
        assert_eq!((frame.width, frame.height), (160, 120));
        engine.shutdown();
    }

    /// RED — **the frame path does not need a graphics driver** (user ruling
    /// 2026-08-28; §7.42).
    ///
    /// The gate the module note's read-back decision rests on. The release
    /// gate's clean virtual machine has no display driver and falls back to
    /// WARP, and the two things that could have gone wrong there are a device
    /// that refuses `VIDEO_SUPPORT` and a `TransferVideoFrame` that has nothing
    /// to transfer onto. [`Adapter::Software`] forces exactly that machine's
    /// conditions onto this one.
    ///
    /// MUTATION: delete the second, flag-less attempt in [`create_device`] and
    /// this goes red on a machine whose WARP does not offer video support —
    /// which is the failure that would otherwise be found by a user on a virtual
    /// machine and by nobody here.
    #[test]
    fn a_software_adapter_still_serves_frames() {
        let _ledger = ledger_gate();
        let mut engine = Engine::open_on(&fixture("folio-video-test.mp4"), Adapter::Software)
            .expect("a WARP engine opens");
        assert!(
            engine.wait_for_metadata(Duration::from_secs(10)),
            "metadata: {:?}",
            engine.state()
        );
        // Asked once the metadata is in and not on the line after the open: the
        // device is chosen on the engine's own thread, and since the open stopped
        // waiting for that thread there is nothing to have chosen it yet.
        assert_eq!(engine.adapter_in_use(), Some(Adapter::Software));
        engine.set_muted(true);
        engine.play();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut frame = None;
        while Instant::now() < deadline && frame.is_none() {
            frame = engine.frame();
            std::thread::sleep(Duration::from_millis(4));
        }
        let frame = frame.expect("WARP serves frames like any other device");
        assert_eq!((frame.width, frame.height), (160, 120));
        engine.shutdown();
    }

    /// RED — **every engine is shut down before the process leaves** (user
    /// ruling 2026-08-28; §7.42, and §7.35's exit protocol).
    ///
    /// A counting gate, because the property is about a *process* and no single
    /// test can watch one end. What it pins is the invariant that makes the
    /// property hold: the two counters move together, so the number of engines
    /// alive returns to what it was however an [`Engine`] goes — an explicit
    /// [`Engine::shutdown`], a drop at the end of a scope, or an unwind past
    /// one.
    ///
    /// The third arm is the one that matters. A `Shutdown` that only ran on the
    /// explicit call would leak a decoder, a work queue and a Direct3D device
    /// for every pane that was closed by anything other than its own button —
    /// and `MFShutdown` at exit would then run with engines still standing,
    /// which is the shape §7.35 spent a whole slice on.
    ///
    /// MUTATION: remove `impl Drop for Engine` and the second and third arms go
    /// red while the first stays green.
    #[test]
    fn every_engine_is_shut_down_before_the_process_leaves() {
        let _ledger = ledger_gate();
        let path = fixture("folio-video-test.mp4");
        let before = engines_outstanding();

        let mut explicit = Engine::open(&path).expect("an engine opens");
        assert_eq!(engines_settling_to(before + 1), before + 1);
        explicit.shutdown();
        assert_eq!(engines_outstanding(), before, "an explicit shutdown counts");

        {
            let _dropped = Engine::open(&path).expect("an engine opens");
            assert_eq!(engines_settling_to(before + 1), before + 1);
        }
        assert_eq!(engines_outstanding(), before, "a drop counts");

        // The third arm has to be sure an engine was actually standing when the
        // unwind began, or "the count came back" would be true of a panic that
        // beat the engine thread to it — and the mutation this arm exists for
        // would pass. The number is carried out rather than asserted inside,
        // because an assertion inside a `catch_unwind` is a panic the
        // `catch_unwind` swallows.
        let standing = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&standing);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _engine = Engine::open(&path).expect("an engine opens");
            counted.store(engines_settling_to(before + 1), Ordering::Relaxed);
            panic!("a pane whose window went away");
        }));
        assert!(unwound.is_err());
        assert_eq!(
            standing.load(Ordering::Relaxed),
            before + 1,
            "an engine was standing when the unwind started"
        );
        assert_eq!(engines_outstanding(), before, "an unwind counts");
        assert!(engines_started() >= 3);
    }

    /// RED — **the type table and the decoder disagree, and the decoder is the
    /// one that is right** (measured 2026-08-28; `docs/DESIGN.md` §7.42).
    ///
    /// This began as a matrix and turned into a finding, so it is written as the
    /// finding. `IMFMediaEngine::CanPlayType` is the platform's *MIME registry*
    /// speaking, not its decoders: on this machine it answers `No` to
    /// `video/quicktime`, `video/x-matroska`, `video/avi` and `video/x-ms-wmv` —
    /// and Media Foundation ships an AVI source, an ASF source, and an MPEG-4
    /// file source that reads `.mov` perfectly well, which
    /// [`a_quicktime_file_plays_where_the_browser_would_not_open_it`] and the
    /// video probe's own screenshot both show it doing.
    ///
    /// **So the matrix in the report is built by opening files, not by asking
    /// this.** What this is still good for is the positive half — a type it
    /// answers `Maybe` or `Probably` to is one it has a decoder registered for,
    /// which is how VP9 and HEVC can be reported per machine without shipping a
    /// fixture in every codec — and for pinning the disagreement itself, which is
    /// the trap the next reader would otherwise fall into. §7.16 asked the
    /// *browser* this same question and treated the empty string as final; here
    /// it is not.
    ///
    /// Only the two structural claims are asserted. Everything else is printed,
    /// because it is a fact about one machine: VP9, AV1 and HEVC depend on Store
    /// extensions this repository does not install, and a gate that demanded
    /// them would be red on a stock Windows and green on a developer's.
    ///
    /// RED GATE: read the matrix off this function instead of off a real open
    /// and `.mov` leaves the class again — which is precisely the mistake
    /// §7.23 (f) had to be talked out of once already.
    #[test]
    fn the_type_table_under_reports_what_the_decoder_will_open() {
        let _ledger = ledger_gate();
        let types = [
            "video/mp4",
            "video/mp4; codecs=\"avc1.42E01E\"",
            "video/mp4; codecs=\"hvc1\"",
            "video/x-m4v",
            "video/quicktime",
            "video/webm",
            "video/webm; codecs=\"vp8\"",
            "video/webm; codecs=\"vp9\"",
            "video/webm; codecs=\"av01.0.04M.08\"",
            "video/x-matroska",
            "video/avi",
            "video/x-ms-wmv",
            "audio/mpeg",
            "audio/mp4",
            "audio/wav",
            "audio/flac",
        ];
        let answers = can_play_types(&types);
        for (kind, answer) in types.iter().zip(&answers) {
            eprintln!("VIDEO_CAN_PLAY {kind:40} {answer:?}");
        }
        let answer_for = |wanted: &str| {
            types
                .iter()
                .position(|kind| *kind == wanted)
                .map(|at| answers[at])
                .expect("a type this test asked about")
        };
        // ① The probe works at all. Every Windows decodes H.264 in MP4, so a
        // `No` here is a broken question rather than an unusual machine.
        assert_ne!(
            answer_for("video/mp4"),
            CanPlay::No,
            "H.264 in MP4 is not optional on this platform"
        );
        // ② The disagreement, pinned. The type table says no and the decoder
        // says yes, and the second is the one the product believes.
        assert_eq!(
            answer_for("video/quicktime"),
            CanPlay::No,
            "if this ever becomes a `Maybe`, the note above is out of date"
        );
        let mut engine = Engine::open(&fixture("folio-video-test.mov"))
            .expect("the same platform opens the file the table refused");
        assert!(
            engine.wait_for_metadata(Duration::from_secs(10)),
            "a type answered `No` still loaded: {:?}",
            engine.state()
        );
        assert_eq!(engine.state().natural_size, Some((160, 120)));
        engine.shutdown();
    }

    /// PIN — **a file that is not a video is refused, and nothing panics.**
    ///
    /// Three refusals with three different mechanisms behind them: a path that
    /// is not there, a file with no bytes, and a file whose bytes are text under
    /// a video's name. The first two may fail at `SetSource` and the third
    /// reaches the source resolver and is turned away by it — so the assertion
    /// is not on *where* it failed but on the one thing a caller cares about:
    /// there is an error and there is no picture.
    ///
    /// MUTATION: `unwrap` any of the `map_err`s in [`Machinery::build`] and the
    /// third case takes a pane's thread down.
    #[test]
    fn nothing_that_is_not_a_video_plays() {
        let _ledger = ledger_gate();
        let dir = std::env::temp_dir().join(format!(
            "folio-video-engine-refusals-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let text = dir.join("renamed.mp4");
        std::fs::write(&text, b"this is not a video at all, whatever it is called")
            .expect("a text file");
        for path in [dir.join("no-such-file.mp4"), text] {
            match Engine::open(&path) {
                Ok(mut engine) => {
                    // Loading is asynchronous, so an engine may exist for a file
                    // that cannot be loaded; what may not happen is a picture.
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while Instant::now() < deadline && engine.state().error.is_none() {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    assert!(
                        engine.state().error.is_some(),
                        "{} loaded: {:?}",
                        path.display(),
                        engine.state()
                    );
                    assert!(
                        engine.frame().is_none(),
                        "{} drew a picture",
                        path.display()
                    );
                    engine.shutdown();
                }
                Err(error) => {
                    assert_ne!(error, EngineError::NoPlatform, "{}", path.display());
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PIN — **pause stops the clock and play starts it again.**
    ///
    /// The four verbs that are not "open" and not "draw", asserted against the
    /// engine's own answers rather than against a copy this module keeps — which
    /// is the whole reason [`EngineState`] is read off the engine every turn of
    /// the loop.
    #[test]
    fn the_verbs_reach_the_engine() {
        let _ledger = ledger_gate();
        let mut engine = Engine::open(&fixture("folio-video-test.mp4")).expect("an engine opens");
        assert!(engine.wait_for_metadata(Duration::from_secs(10)));
        engine.set_muted(true);
        engine.set_volume(0.25);
        engine.set_rate(1.5);
        engine.seek(2.0);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let state = engine.state();
            if state.muted && (state.rate - 1.5).abs() < 0.01 && state.position_secs >= 1.9 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let state = engine.state();
        assert!(state.muted, "{state:?}");
        assert!((state.rate - 1.5).abs() < 0.01, "{state:?}");
        assert!((state.volume - 0.25).abs() < 0.01, "{state:?}");
        assert!(state.position_secs >= 1.9, "the seek landed: {state:?}");

        engine.play();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !engine.state().playing {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(engine.state().playing, "{:?}", engine.state());
        engine.pause();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && engine.state().playing {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!engine.state().playing, "{:?}", engine.state());
        engine.shutdown();
    }
}
