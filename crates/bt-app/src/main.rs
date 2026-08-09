use std::{
    backtrace::Backtrace,
    collections::BTreeMap,
    fs::OpenOptions,
    io::Write,
    num::{NonZeroI64, NonZeroU32, NonZeroUsize},
    ops::{Deref, DerefMut},
    panic,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod input;
mod marks;
mod peek_strip;
mod persist;
mod profiles;
mod restore;
mod seats;
mod seed;
mod settings;
mod tooltip;

use anyhow::{Context, Result, anyhow, ensure};
use bt_doc::{Bias, LayoutKey};
use bt_layout::{
    Axis, LayoutNode, LogicalRect, SeatId, SeatLayout, SeatMetrics, SplitId, WorkAreaHint,
};
use bt_math::{MathEngine, MathRaster, MathRenderError};
use bt_persist::{
    LayoutNodeV1, LeafNodeV1, SESSION_SCHEMA_VERSION, SessionCursorStyleV1, SessionThemeV1,
    SessionV1, TabV1, TermLeafV1, ThemeModeV1, WindowBoundsV1, WindowStateV1,
};
use bt_pty::{OutputWake, PSREADLINE_INVOKE_PROMPT_INPUT, PtySession, PtySize};
use bt_render::{
    ChromePalette, CursorStyle, FrameSource, FrameTrigger, GridSize, ImeCursorArea,
    LatestFrameSlot, MathHit, MathHitTarget, PREVIEW_BODY_INSET_LOGICAL_PX, PeekImageOverlay,
    Preedit, PresentOutcome, PreviewImage, Renderer, SeatViewport, Theme, ThemeChange,
    WINDOW_TAB_BREATHE_MIN_OPACITY, WINDOW_TAB_BREATHE_PERIOD_MS,
    WINDOW_TAB_BREATHE_REDUCED_OPACITY, WINDOW_TAB_PIN_REVEAL_MS,
    WINDOW_TAB_RING_INDETERMINATE_TURNS, WINDOW_TAB_RING_SPIN_PERIOD_MS,
    WINDOW_TAB_RING_SWEEP_TRANSITION_MS, background_rgb, compose_preedit, current_cursor_style,
    foreground_rgb, frame_content_digest, frame_is_alternate_screen, preview_image_extent,
    set_cursor_style, set_theme, theme_revision,
};
use bt_term::{
    DualPlaneSession, InlineImageDecoder, MathLayoutOptions, MouseTracking, ProgressState,
    SessionDecorationTask, SessionMathTask, SessionStatus, TerminalModes,
    normalized_local_image_path_key, render_detection_task, render_live_detection_task,
};
use bt_transcript::DEFAULT_STAGING_QUOTA;
use bt_viewport::{
    HyperlinkHit, MathBlockAnchor, ViewSelection, ViewportFrame, ViewportProjection,
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState, NamedKey},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Theme as OsTheme, Window, WindowId},
};

const INITIAL_WIDTH: f64 = 960.0;
const INITIAL_HEIGHT: f64 = 600.0;
const DEFAULT_PROFILE_TITLE: &str = "PowerShell";
#[cfg(test)]
const WINDOW_TITLE: &str = "BetterTerminal M0-beta";
const WIN32_DEFAULT_DPI: f64 = 96.0;
const STARTUP_PTY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
/// One 60 Hz frame: coalesce cursor-area churn without leaving the final position unsent.
const IME_CURSOR_AREA_INTERVAL: Duration = Duration::from_millis(16);
/// The mock-up's `.cursor` uses a 1.1 second step-end animation, so each visible/hidden phase is
/// half of that cycle.
const CURSOR_BLINK_PHASE: Duration = Duration::from_millis(550);
/// How often a live tab-strip animation asks to be redrawn.
///
/// 60Hz, and only ever while something is actually moving — a breath, a
/// spinning indeterminate arc, or an arc easing to a new reading. The strip
/// reports no deadline at all otherwise, so this is the *rate* of an animation
/// and never a standing poll.
///
/// The budget it has to fit in is small and known: an animating ring costs one
/// rasterize of a 15px SVG per frame, measured at 16.5µs on this workspace's
/// own rasterizer — a tenth of a percent of a frame, per ring.
const STRIP_ANIMATION_FRAME: Duration = Duration::from_millis(16);
/// Winit 0.30 has no enter/exit-size-move event; the final ConPTY size is committed after this
/// silence interval while the local surface and terminal grid continue to follow every event.
const WINDOW_RESIZE_QUIET: Duration = bt_term::RESIZE_REQUEST_QUIET;
/// Whether a live OSC 133 input buffer withholds terminal and ConPTY resizes (user ruling
/// 2026-08-06). **A policy bit, not a structure.**
///
/// `false` lets every resize through the existing 200 ms coalescer even while input is present.
/// The post-commit private shell-integration injection remains active, so PSReadLine repairs its
/// cached anchor at every landed width. Empty buffers use the integration's output-free re-anchor;
/// non-empty buffers retain InvokePrompt. Flipping this back to `true` restores the retained
/// confirm-then-release machine verbatim; `typed_input_defers_resize` is the single production
/// convergence point.
const TYPED_INPUT_RESIZE_DEFERRAL: bool = false;
/// M0-alpha's single-session frozen-line budget; later configuration work may expose it.
const M0_FROZEN_LINE_QUOTA: NonZeroUsize = NonZeroUsize::new(100_000).unwrap();
const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const HYPERLINK_HOVER_DELAY: Duration = Duration::from_millis(300);
const MATH_WORKER_STOPPED_NOTICE: &str =
    "Formula rendering stopped; terminal input and output remain available";
const PANIC_LOG_FILENAME: &str = "bt-app-panic.log";

#[derive(Clone, Copy, Debug)]
enum AppEvent {
    PtyOutput,
    MathReady,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TabId(u64);

struct MathWorkerResult {
    tab_id: TabId,
    completion: DecorationWorkerCompletion,
}

enum MathWorkerRequest {
    Math {
        tab_id: TabId,
        task: Box<SessionMathTask>,
        foreground_rgb: [u8; 3],
    },
    InlineImage {
        tab_id: TabId,
        task: bt_term::InlineImageTask,
    },
    /// Hover-peek decode: read and decode a local image off-thread without touching any
    /// decoration record. The completion routes only to the app-side peek cache, so the band
    /// creation gates (cursor line, semantic input region) are never bypassed.
    PeekImage { tab_id: TabId, path: PathBuf },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScalePurpose {
    InlineImage(u64),
    Peek,
    Preview,
}

enum ScaleWorkerRequest {
    InlineImage {
        tab_id: TabId,
        task: bt_term::InlineImageScaleTask,
    },
    Peek {
        tab_id: TabId,
        task: bt_term::InlineImageScaleTask,
    },
    Preview {
        tab_id: TabId,
        task: bt_term::InlineImageScaleTask,
    },
}

impl ScaleWorkerRequest {
    fn purpose(&self) -> ScalePurpose {
        match self {
            Self::InlineImage { task, .. } => ScalePurpose::InlineImage(task.occurrence_id),
            Self::Peek { .. } => ScalePurpose::Peek,
            Self::Preview { .. } => ScalePurpose::Preview,
        }
    }

    fn task(&self) -> &bt_term::InlineImageScaleTask {
        match self {
            Self::InlineImage { task, .. }
            | Self::Peek { task, .. }
            | Self::Preview { task, .. } => task,
        }
    }

    fn tab_id(&self) -> TabId {
        match self {
            Self::InlineImage { tab_id, .. }
            | Self::Peek { tab_id, .. }
            | Self::Preview { tab_id, .. } => *tab_id,
        }
    }

    fn same_target(&self, other: &Self) -> bool {
        self.tab_id() == other.tab_id()
            && self.purpose() == other.purpose()
            && self.task().content_key == other.task().content_key
    }

    fn completion(self) -> (TabId, DecorationWorkerCompletion) {
        match self {
            Self::InlineImage { tab_id, task } => (
                tab_id,
                DecorationWorkerCompletion::ScaleInlineImage {
                    scaled: bt_term::scale_inline_image(&task),
                },
            ),
            Self::Peek { tab_id, task } => (
                tab_id,
                DecorationWorkerCompletion::PeekScaledImage {
                    scaled: bt_term::scale_inline_image(&task),
                },
            ),
            Self::Preview { tab_id, task } => (
                tab_id,
                DecorationWorkerCompletion::PreviewScaledImage {
                    scaled: bt_term::scale_inline_image(&task),
                },
            ),
        }
    }
}

#[derive(Default)]
struct PendingScaleRequests {
    requests: std::collections::VecDeque<ScaleWorkerRequest>,
}

impl PendingScaleRequests {
    fn push_latest(&mut self, request: ScaleWorkerRequest) {
        if let Some(index) = self
            .requests
            .iter()
            .position(|queued| queued.same_target(&request))
        {
            self.requests.remove(index);
        }
        self.requests.push_back(request);
    }

    fn pop_front(&mut self) -> Option<ScaleWorkerRequest> {
        self.requests.pop_front()
    }

    fn contains_target(&self, request: &ScaleWorkerRequest) -> bool {
        self.requests
            .iter()
            .any(|queued| queued.same_target(request))
    }

    fn drain_channel(&mut self, receiver: &mpsc::Receiver<ScaleWorkerRequest>) {
        while let Ok(request) = receiver.try_recv() {
            self.push_latest(request);
        }
    }
}

/// Run the CPU-heavy resample lane. Before each Lanczos3 pass, absorb every request already in the
/// channel and discard a dequeued question if a newer size for the same content/purpose exists.
/// A request that arrives after the pass begins is a new question and will be serviced next.
fn run_scale_worker(
    receiver: mpsc::Receiver<ScaleWorkerRequest>,
    mut execute: impl FnMut(ScaleWorkerRequest),
) {
    let mut pending = PendingScaleRequests::default();
    while let Ok(request) = receiver.recv() {
        pending.push_latest(request);
        pending.drain_channel(&receiver);
        while let Some(request) = pending.pop_front() {
            pending.drain_channel(&receiver);
            if pending.contains_target(&request) {
                continue;
            }
            execute(request);
        }
    }
}

enum DecorationWorkerCompletion {
    Math {
        task: Box<SessionMathTask>,
        result: std::result::Result<MathRaster, MathRenderError>,
    },
    InlineImage {
        task: bt_term::InlineImageTask,
        result: std::result::Result<bt_term::DecodedInlineImage, bt_term::InlineImageDecodeError>,
    },
    /// A decoded image resampled into the display box a band shows it in. Resampling a
    /// wallpaper-sized decode is tens of milliseconds, so it belongs here and never on the event
    /// thread.
    ScaleInlineImage {
        scaled: bt_term::ScaledInlineImage,
    },
    PeekImage {
        path: PathBuf,
        result: std::result::Result<bt_term::DecodedInlineImage, bt_term::InlineImageDecodeError>,
    },
    PeekScaledImage {
        scaled: bt_term::ScaledInlineImage,
    },
    PreviewScaledImage {
        scaled: bt_term::ScaledInlineImage,
    },
}

struct MathWorker {
    tasks: mpsc::Sender<MathWorkerRequest>,
    scale_tasks: mpsc::Sender<ScaleWorkerRequest>,
    results: mpsc::Receiver<MathWorkerResult>,
}

impl MathWorker {
    fn spawn(proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let (task_tx, task_rx) = mpsc::channel::<MathWorkerRequest>();
        let (scale_tx, scale_rx) = mpsc::channel::<ScaleWorkerRequest>();
        let (result_tx, result_rx) = mpsc::channel::<MathWorkerResult>();
        let scale_result_tx = result_tx.clone();
        let scale_proxy = proxy.clone();
        thread::Builder::new()
            .name("bt-image-scale-worker".to_owned())
            .spawn(move || {
                run_scale_worker(scale_rx, |request| {
                    let (tab_id, completion) = request.completion();
                    if scale_result_tx
                        .send(MathWorkerResult { tab_id, completion })
                        .is_ok()
                    {
                        let _ = scale_proxy.send_event(AppEvent::MathReady);
                    }
                });
            })
            .context("spawn image resampling worker")?;
        thread::Builder::new()
            .name("bt-math-worker".to_owned())
            .spawn(move || {
                let engine = MathEngine::new();
                let mut image_decoder = InlineImageDecoder::default();
                while let Ok(work) = task_rx.recv() {
                    let completion = match work {
                        MathWorkerRequest::Math {
                            tab_id,
                            task,
                            foreground_rgb,
                        } => (
                            tab_id,
                            match *task {
                                SessionMathTask::Frozen(mut task) => {
                                    let result =
                                        render_detection_task(&engine, &mut task, foreground_rgb);
                                    DecorationWorkerCompletion::Math {
                                        task: Box::new(SessionMathTask::Frozen(task)),
                                        result,
                                    }
                                }
                                SessionMathTask::Live(mut task) => {
                                    let result = render_live_detection_task(
                                        &engine,
                                        &mut task,
                                        foreground_rgb,
                                    );
                                    DecorationWorkerCompletion::Math {
                                        task: Box::new(SessionMathTask::Live(task)),
                                        result,
                                    }
                                }
                            },
                        ),
                        MathWorkerRequest::InlineImage { tab_id, task } => {
                            let result = image_decoder.decode(task.clone());
                            (
                                tab_id,
                                DecorationWorkerCompletion::InlineImage { task, result },
                            )
                        }
                        MathWorkerRequest::PeekImage { tab_id, path } => {
                            let result = image_decoder.decode(bt_term::InlineImageTask {
                                occurrence_id: 0,
                                source: bt_term::InlineImageSource::LocalPath(path.clone()),
                            });
                            (
                                tab_id,
                                DecorationWorkerCompletion::PeekImage { path, result },
                            )
                        }
                    };
                    if result_tx
                        .send(MathWorkerResult {
                            tab_id: completion.0,
                            completion: completion.1,
                        })
                        .is_err()
                    {
                        break;
                    }
                    let _ = proxy.send_event(AppEvent::MathReady);
                }
            })
            .context("spawn math rendering worker")?;
        Ok(Self {
            tasks: task_tx,
            scale_tasks: scale_tx,
            results: result_rx,
        })
    }
}

fn disable_math_worker_state(running: &mut bool, notice_pending: &mut bool) -> bool {
    if !*running {
        return false;
    }
    *running = false;
    *notice_pending = true;
    eprintln!("formula rendering worker stopped; terminal input and output remain available");
    true
}

fn take_math_worker_notice(notice_pending: &mut bool) -> Option<&'static str> {
    if std::mem::take(notice_pending) {
        Some(MATH_WORKER_STOPPED_NOTICE)
    } else {
        None
    }
}

fn dispatch_decoration_task(
    tab_id: TabId,
    task: SessionDecorationTask,
    tasks: &mpsc::Sender<MathWorkerRequest>,
    scale_tasks: &mpsc::Sender<ScaleWorkerRequest>,
) -> bool {
    match task {
        SessionDecorationTask::Math(task) => tasks
            .send(MathWorkerRequest::Math {
                tab_id,
                task,
                foreground_rgb: foreground_rgb(),
            })
            .is_ok(),
        SessionDecorationTask::InlineImage(task) => tasks
            .send(MathWorkerRequest::InlineImage { tab_id, task })
            .is_ok(),
        SessionDecorationTask::ScaleInlineImage(task) => scale_tasks
            .send(ScaleWorkerRequest::InlineImage { tab_id, task })
            .is_ok(),
    }
}

/// Drain the real session queue into the renderer channel. A dead optional-decoration worker is
/// a one-way feature downgrade, never a terminal/runtime error.
fn dispatch_pending_math_tasks(
    tab_id: TabId,
    session: &mut DualPlaneSession,
    tasks: &mpsc::Sender<MathWorkerRequest>,
    scale_tasks: &mpsc::Sender<ScaleWorkerRequest>,
    running: &mut bool,
    notice_pending: &mut bool,
) -> bool {
    if !*running {
        return false;
    }
    while let Some(task) = session.take_decoration_worker_task() {
        if !dispatch_decoration_task(tab_id, task, tasks, scale_tasks) {
            return disable_math_worker_state(running, notice_pending);
        }
    }
    false
}

#[derive(Clone, Copy, Debug)]
struct DpiSnapshot {
    winit_scale: f64,
    win32_dpi: u32,
    authoritative_scale: f64,
    rect: bt_platform::WindowRect,
}

/// Everything whose identity follows a tab rather than the native window.
///
/// `Runtime` dereferences to its active entry so the single-tab hot path keeps using the exact
/// same code. Background entries are only touched by the explicit PTY/timeout drains; they never
/// publish into the window's frame slot.
/// One shell, and everything that is true of that shell and nothing else.
///
/// A tab used to *be* one of these. U12 splits the two apart: a tab is a strip
/// entry with a layout tree, and every Terminal leaf of that tree owns one of
/// these — its own ConPTY, its own screen, its own projection, its own idea of
/// how many columns it has.
///
/// Nothing in here knows its own `SeatId`. The mapping lives in
/// [`TabState::sessions`], which is what keeps red line L1 intact from the other
/// direction: the layout tree carries no session, and the session carries no
/// seat, and `bt-app` — the one crate allowed to know both — holds the pairing.
struct LeafSession {
    pty: Option<PtySession>,
    session: DualPlaneSession,
    shell_fallback_notice: Option<String>,
    projection: ViewportProjection,
    grid: GridSize,
    conpty_grid: GridSize,
    pending_pty_resize: Option<PendingPtyResize>,
    pending_psreadline_resize_reanchor: bool,
    /// The revision this leaf's session had reached the last time the user was
    /// looking at it — the whole of what "unread" is measured against.
    ///
    /// Per leaf because it is measured against a *session's* revision counter,
    /// and two shells count their own. The tab's badge is the aggregate of these
    /// (D34: the tab takes its loudest member's claim), not a separate tally.
    last_seen_revision: u64,
    /// The frame this leaf last put on the glass.
    ///
    /// Every pointer question is asked of a *frame* — which cell is under the
    /// pointer, does that cell wear a hyperlink, is there a math block there —
    /// and with several panes on screen the honest answer depends on which pane
    /// the pointer is in. Keeping each leaf's own frame is what lets a hover
    /// over the right-hand pane be answered by the right-hand pane's cells
    /// instead of by whichever pane happens to hold the keyboard.
    last_presented_frame: Option<ViewportFrame>,
}

struct TabState {
    id: TabId,
    /// This tab's shells, one per Terminal leaf, keyed by the seat they draw
    /// into and ordered by it.
    ///
    /// A `BTreeMap` rather than a `HashMap` for the reason L8 puts on the
    /// solver's own output: iteration order reaches the screen — it decides
    /// which seat's frame is built first and which shell drains first — and
    /// hash order is not an order anyone chose.
    ///
    /// The invariant every access below relies on: this is never empty, and it
    /// always contains `focused_leaf`. A Terminal seat with no session behind it
    /// is a black rectangle, so seats and sessions are created and destroyed
    /// together.
    sessions: BTreeMap<SeatId, LeafSession>,
    /// Which leaf has the keyboard. Typing, pasting and IME all land here.
    focused_leaf: SeatId,
    /// Which of [`profiles::PROFILES`] started this tab — the stable half of its
    /// seed, kept so a closed tab can be reopened as the same *kind* of shell
    /// rather than whatever the default happens to be that day.
    profile: usize,
    /// "Bring this one back next time."
    ///
    /// It is an answer, not a decoration, and that is why launch does not ask
    /// about a pinned tab: you already told it (mock-up 7426-7431). Three things
    /// follow from the flag and the mock-up insists on all three — the tab leads
    /// the strip, it has no `×`, and only then does it wear a solid pin.
    pinned: bool,
    /// The name the user typed for this tab, overriding every layer under it.
    ///
    /// "`name` is an OVERRIDE, not a field, and that single fact designs the
    /// whole editor: clearing it does not blank the tab, it REVEALS the layer
    /// underneath" (mock-up line 2595). The slot and its precedence land here;
    /// the editor that writes to it is the rename ticket's, so today nothing
    /// sets it and every tab reads through to what the program or the shell
    /// said.
    manual_name: Option<String>,
    pending_keyboard_at: Option<Instant>,
    pending_resize_present: Option<GridSize>,
    seats: seats::Seats,
    seat_layout: SeatLayout,
    /// What the L4 fit-what-fits strip could not show, when the last solve
    /// landed there. Stored beside the layout because it is the other half of
    /// the same answer and is derived by the same pass (§4.3).
    seat_overflow: Option<seats::FitOverflow>,
    preview_image: Option<PreviewImageState>,
    /// The arc's easing toward the reading it is now showing, if it is moving.
    ring_tween: Option<SweepTween>,
    /// The sweep the ring is displaying, which is also what a state change
    /// arriving without a percentage keeps.
    ring_sweep: Option<u16>,
    /// The pin's hover reveal. A pinned tab holds it open at 1.0; an unpinned
    /// one opens it while the pointer is on the tab and closes it after.
    pin_reveal: RevealTween,
    /// The reveal the strip was last told to draw, quantised — the pin's half of
    /// the frame debt.
    last_drawn_pin_reveal: Option<u8>,
    /// The mark state the strip was last told to draw for this tab.
    ///
    /// The scheduler's own record, and the whole of how it notices that a
    /// channel has *stopped* — a thing no "is it moving?" test can ever see,
    /// because by the time it matters, nothing is.
    last_drawn_mark: Option<seats::TabMarkState>,
    /// When this tab's mark-slot animations started counting.
    ///
    /// Per tab rather than per window, which is what CSS does: an animation
    /// begins when its element does, so two tabs opened a second apart breathe
    /// a second out of step. A single window-wide clock would have every tab
    /// pulsing in lockstep, which reads as one mechanism rather than as several
    /// sessions each doing their own work.
    animation_epoch: Instant,
    /// This tab's slide back to its own slot: the FLIP a displaced neighbour
    /// runs, and the settle the tab you let go of runs, which are the same
    /// motion started from two different places.
    flip: FlipTween,
    /// The landing wash, running down from 1.0 the moment this tab comes to rest
    /// after a drag.
    landing: LandTween,
    /// How far from its slot the strip was last told to draw this tab, rounded
    /// to whole physical pixels — the drag's half of the frame debt.
    ///
    /// Rounded for the reason the pin's reveal is quantised: a pixel is the
    /// finest difference that can reach the glass, and a tween settling in the
    /// last thousandth of one would otherwise owe a frame forever.
    last_drawn_offset: Option<i32>,
    /// The landing wash the strip was last told to draw, quantised to the 1/255
    /// a sprite's opacity resolves to.
    last_drawn_landing: Option<u8>,
}

struct Runtime {
    renderer: Renderer,
    tabs: Vec<TabState>,
    active_tab: usize,
    next_tab_id: u64,
    event_proxy: EventLoopProxy<AppEvent>,
    math_worker: MathWorker,
    math_worker_running: bool,
    math_worker_notice_pending: bool,
    /// One-shot startup notice from `PtySession::spawn_default` falling back to `powershell.exe`.
    /// Shown on the first frame published, then discarded — see `shell_fallback_notice` at the
    /// `spawn_default` call site.
    pending_frames: LatestFrameSlot,
    /// The size the child has actually been told about — never a size that has only been solved or
    /// queued. It is the same value as `grid` at rest, and the two are deliberately separate only
    /// where they genuinely differ: inside the `WINDOW_RESIZE_QUIET` coalescing window our grid has
    /// already reflowed while the child has not heard yet, and under the typed-input deferral
    /// (`schedule_grid_change`) neither has moved but a target is queued. Asking "does the child
    /// need to hear this?" of our own grid answers that question with the wrong fact, and a drag
    /// that comes back to where the child already sits would then still send it a resize.
    modifiers: ModifiersState,
    math_context_menu: bt_platform::MathContextMenu,
    custom_window_frame: bt_platform::CustomWindowFrame,
    window: Arc<Window>,
    startup_started: Instant,
    trace_startup: bool,
    trace_resize: bool,
    trace_layout_events: bool,
    /// The geometry changes the most recent layout commit produced (T230).
    ///
    /// An outbox, replaced whole at each commit rather than appended to, because
    /// "exactly one batch per commit" is the contract and an accumulating list
    /// would let two commits arrive looking like one. A commit that changed
    /// nothing leaves it empty, which is the honest report and not an omission.
    ///
    /// Nobody reads it yet beyond the trace. `M2-tiny-window-priority.md` §3.5
    /// names the consumer — a TRANSIENT overlay anchored inside a seat, which
    /// dissolves when its anchor rectangle stops being the one it was laid out
    /// against — and the floating-window slice owns it. Publishing first is
    /// deliberate: the four facts are only visible from inside this block, and a
    /// consumer written later against an interface that does not exist yet is
    /// how the mapping gets invented twice.
    last_layout_events: Vec<seats::LayoutEvent>,
    trace_perf: bool,
    resize_trace_logged_transaction: u64,
    resize_trace_logged_events: usize,
    background_visible: Option<Duration>,
    first_text_visible: Option<Duration>,
    window_shown: bool,
    first_visible_present_dpi_checked: bool,
    first_text_presented: bool,
    last_presented_frame: Option<ViewportFrame>,
    preedit: Option<Preedit>,
    ime_active: bool,
    ime_cursor_throttle: ImeCursorThrottle,
    cursor_blink: CursorBlink,
    /// Whether the window holds focus.
    ///
    /// A window nobody is looking at consumes nothing: this is the second half
    /// of [`attention_is_consumed`], and the reason a bell that rings while the
    /// user is away in another application is still waiting when they return.
    window_focused: bool,
    /// Whether this system wants animation at all, read once at start-up.
    ///
    /// Once, because it is an accessibility preference rather than a live
    /// signal: Windows broadcasts `WM_SETTINGCHANGE` when it moves, and until
    /// this window listens for that, re-reading it every frame would buy a
    /// system call per frame and no extra correctness.
    motion: Motion,
    ime_system_caret: bt_platform::ImeSystemCaret,
    pointer_position: Option<PhysicalPosition<f64>>,
    mouse_route: Option<MouseRoute>,
    click_tracker: ClickTracker,
    line_wheel_remainder: f64,
    pixel_wheel_remainder: f64,
    /// Wheel detents awaiting a mouse-protocol report; per-notch, no system-lines multiplier.
    notch_wheel_remainder: f64,
    /// Fractional subpixels awaiting consumption by the LOCAL scroll routes only. Forwarding
    /// routes keep their own line-quantized accumulators above; the two never pour into each
    /// other, so switching routes cannot dump parked residue as a sudden jump.
    local_wheel_subpixel_remainder: f64,
    /// One private PSReadLine anchor repair owed by the current resize transaction. ConPTY cursor
    /// reads are a terminal round trip (`CSI 6 n` -> CPR), so writing the chord beside
    /// `PtySession::resize` can make the handler sample a cursor the child has not settled yet.
    /// A boolean is intentional: every commit in one drag replaces the same debt, and only the
    /// final transaction quiescence may pay it.
    hyperlink_hover: HyperlinkHover,
    /// Every image reference the frame currently on screen draws, as the cells that draw it — the
    /// session's scan of that frame, kept because the four verbs must read the *same* list the paint
    /// read (user ruling 2026-08-04). Rescanning on each pointer event would be a second opinion
    /// about a frame that has not changed; this is the first one, held.
    frame_image_references: FrameImageReferences,
    /// The reference the frame on screen was drawn with the solid underline over. Read only to
    /// decide whether the pointer has moved onto or off a reference and a repaint is therefore owed.
    underlined_image_reference: Option<bt_term::FrameImageReference>,
    peek_hover: PeekHover,
    peek_cache: std::collections::HashMap<String, PeekCacheEntry>,
    /// The one display-sized thumbnail the flyout can draw, and the one resample in flight. See
    /// `PeekThumbnail` for why a single entry is the whole policy.
    peek_thumbnail: Option<PeekThumbnail>,
    peek_thumbnail_pending: Option<PeekThumbnailTarget>,
    math_hover_anchor: Option<MathBlockAnchor>,
    math_hover_clear_at: Option<Instant>,
    pending_math_context_anchor: Option<MathBlockAnchor>,
    /// The layout tree this window hosts. A lone terminal leaf by default, which
    /// is today's window written down.
    /// The most recent answer from `solve`. Every rectangle the renderer and the
    /// input router use comes from here, so the picture and the hit test can
    /// never be two geometries (D4).
    seat_pointer: seats::ChromePointer,
    /// The one tooltip this window has (M136-M142). A singleton because a tip
    /// answers "what is this?" about the thing under the pointer, and there is
    /// one pointer.
    tooltip: tooltip::TooltipHost,
    /// The layout peek (T7) — the tip's near relative, and deliberately not the
    /// same singleton.
    ///
    /// Two hosts because the two disagree about both things a hover popup is: a
    /// 350ms clock against the tip's 380ms, and no fade at all against the tip's
    /// 90ms. Folding them together would have left one host with two delays and
    /// a conditional fade, and then §6's mutual exclusion — the reason they are
    /// apart — would have had nothing to exclude. See [`peek_strip`].
    layout_peek: peek_strip::PeekHost,
    /// Everything tippable, rebuilt beside the chrome it describes.
    ///
    /// Never cached across a rebuild: the mock-up rewrites `el.title` on every
    /// paint (line 4331), and the reason is that every one of these strings is a
    /// function of live state — a name that can be edited, a folder that can be
    /// walked, a percentage that climbs. A tip that outlived its rebuild would be
    /// a confident sentence about a tab that stopped being that tab.
    tooltip_anchors: tooltip::TooltipAnchors,
    /// The opacity the tip was last *painted* at, or `None` when none was.
    ///
    /// The strip's own frame-debt idea (`tab_owes_frame`) applied to the fade:
    /// what is on screen, compared against what should be, rather than a flag
    /// saying an animation is running. The difference shows at exactly one
    /// instant and it is the one that matters — the frame the fade lands on,
    /// which "is it still fading" answers `false` for and therefore never draws.
    tooltip_drawn_opacity: Option<f32>,
    /// Rasterized chrome marks, held across frames so a hover repaint costs a
    /// hash lookup rather than eight SVG renders.
    chrome_marks: marks::ChromeMarkRasters,
    /// Whether the settings dialog is up, and what is open inside it.
    ///
    /// App state, deliberately beside the layout rather than in it: a dialog is
    /// not a seat, so the solver never hears about it, and it is not an intent,
    /// so the session file never does either — a window that reopened with a
    /// question on it would be answering one nobody asked.
    settings: settings::SettingsPanel,
    /// Whether the tab strip's profile picker is up. Beside `settings` and for
    /// the same reasons — and separate from it because the two are different
    /// kinds of surface: one is modal and one is a popup.
    profile_menu: profiles::ProfileMenu,
    /// The picker's arrow on its way to matching it.
    ///
    /// Beside `profile_menu` rather than inside it because they are two
    /// different facts on two different clocks: the menu is up or down the
    /// instant it is clicked, and for 140ms after that the arrow is still
    /// travelling. `ProfileMenu` answers hit-testing and layout, which cannot
    /// be told a half-truth; this answers only what the button looks like.
    chevron_turn: ChevronTurn,
    /// The angle that arrow was last actually *drawn* at, in the mark's own
    /// quantized degrees — the chevron's half of the `tab_owes_frame` ledger.
    ///
    /// Compared rather than sampled, for the reason the tab tweens are: a turn
    /// still nominally running but landing on the same raster two frames in a
    /// row has not moved, and a present that redraws the same pixels is a
    /// present nobody asked for. `cubic-bezier(.2,0,0,1)` makes that the common
    /// case rather than a corner one — its tail crawls the last few degrees
    /// over a third of the duration.
    last_drawn_chevron: Option<marks::ChromeMark>,
    /// How far the tab strip is scrolled, in physical pixels (A7/A8).
    ///
    /// App state and nothing else: a scroll offset is where you are looking, not
    /// what you have, so it is not a seat and it is not in the session file. It
    /// is stored unclamped-by-nobody — every read runs it back through
    /// `tab_strip_geometry`, which clamps it to the content that exists right
    /// now, so a resize or a closed tab cannot leave the strip parked past its
    /// own end.
    tab_scroll: f32,
    /// The left press being held on a tab, and what it still owes it (J105).
    ///
    /// One at a time, because a mouse has one left button. It survives the
    /// activation it pays for — a press that has already switched the view is
    /// still a press, and T5's drag needs to find it there.
    tab_press: Option<TabPress>,
    /// The left press being held on a pane head (J118).
    ///
    /// A second field rather than a second variant of [`Self::tab_press`],
    /// because the two hold different things — a tab press carries an unpaid
    /// activation and a pane press carries only the six pixels — and because
    /// they cannot both exist: `chrome_mouse_input` routes one press to one
    /// target, and the one it did not choose is cleared.
    pane_press: Option<PanePress>,
    /// The gesture in flight, whatever it is carrying (J111).
    ///
    /// Separate from the presses rather than a further promise state, because
    /// they answer different questions and outlive each other in both
    /// directions: a press that has not travelled is not a drag, and a drag that
    /// has been cancelled still has to hand the press back its answer (J108).
    ///
    /// `is_some()` is this window's `body.dragging` (J117), and it is read
    /// rather than mirrored: every suppression in the window — the cursor's
    /// shape, the divider's silence, the tip, the peek, the terminal's own
    /// selection — asks this one field, so a source added later is silenced by
    /// all of them without touching any of them.
    drag: Option<Drag>,
    /// The dock drawing on screen — U6's whole state (M144-M155).
    ///
    /// Beside the drag rather than inside it, and for a reason the type system
    /// makes plain: [`Drag`] is `Copy` because everything in it is an identity or
    /// a number, and a plan is a tree and a solved layout. But the arrangement
    /// also says something true — the drawing outlives the answer it draws. When
    /// the pointer leaves every landing the plan stays for as long as the fade
    /// takes to run down, exactly as the mock-up's element does when `.show` comes
    /// off it, and a field inside the drag could not express that.
    drop_preview: Option<DropPreview>,
    /// The dock overlay's opacity as it was last handed to the renderer, in the
    /// 1/255ths a layer's alpha resolves to — the fade's half of the frame debt.
    last_drawn_dock_reveal: Option<u8>,
    /// The rectangle the tree was last solved into (§4.1).
    ///
    /// A window property and not a tab's, which is why it is here and not beside
    /// [`TabState::seat_layout`]: every tab is solved into the same box. It is
    /// *stored* rather than recomputed because a drop plan has to be solved into
    /// the very rectangle the layout it is compared against was solved into —
    /// deriving it a second time from the same inputs is how two numbers that
    /// must be equal acquire a way to differ (A12, T228).
    seat_viewport: LogicalRect,
    /// The strip's double-click history (J99).
    tab_clicks: TabClicks,
    /// The open tab-name editor, or `None` when nobody is renaming anything.
    ///
    /// On the runtime rather than on the tab, because it is a *window*-level
    /// stance and not a property of a session: it is `InputOwner::Rename`
    /// (`docs/DESIGN.md` §7.1.5), there is exactly one keyboard, and two tabs
    /// being renamed at once is not a state this window can be in.
    rename: Option<TabRename>,
    /// The rename caret's own blink, on the same beat as the terminal's.
    ///
    /// Its own instance rather than a share: typing a name must reveal the name
    /// caret, and the two carets answer to different keystrokes. The *phase* is
    /// shared, because there is one `CURSOR_BLINK_PHASE` in this window and a
    /// second beat would read as a second application.
    rename_blink: CursorBlink,
    /// The overlay's own mark rasters. A second cache rather than a share,
    /// because `ChromeMarkRasters::resolve` keeps exactly what the call asked
    /// for: one cache serving two lists would evict each on the other's turn.
    settings_marks: marks::ChromeMarkRasters,
    divider_drag: Option<DividerDrag>,
    /// The last work area that was successfully observed (tiny-window §4.4).
    work_area: WorkAreaHint,
    session_store: persist::SessionStore,
    /// The one store that pin, Recent and undo-close all draw from — kept beside
    /// the tabs rather than inside the session file's mirror so the three doors
    /// read live state, not the last thing that happened to be flushed.
    recent: seed::SeedVault,
    /// Tabs from the last session that were **not** pinned, waiting on the
    /// restore prompt's question. Empty once it has been answered.
    ///
    /// It has to outlive the prompt because closing the window with the question
    /// still open must put these back where they came from (§7.1.4: "restore 提示
    /// 未答复时再关窗：未答复计划并回 lastSession，不得丢失") — an unanswered
    /// question is not a "no".
    pending_restore: Vec<TabV1>,
    /// Whether the "Reopen your other tabs?" question is on screen.
    restore_prompt: restore::RestorePrompt,
    /// The stand-in shell launch opened because nothing was pinned — "a stand-in
    /// for an answer we do not have yet" (mock-up 7464).
    ///
    /// It is removed if the restore prompt is accepted while it is still
    /// untouched (§7.1.4: "restore 接受后占位（未被使用时）移除"). The moment you
    /// type into it, it stops being scaffolding and becomes a tab you are using,
    /// so the first keystroke forgets it here and it is never taken away.
    placeholder_tab: Option<TabId>,
    /// Persisted user choice, distinct from the resolved renderer theme. Under `BT_BG` the process
    /// colors are locked but this mode is still kept across a diagnostic launch.
    theme_mode: ThemeModeV1,
    /// The last aggregate minimum handed to winit. On Windows, winit 0.30 re-applies the current
    /// inner size whenever this setter runs; repeating an unchanged minimum can therefore feed the
    /// non-client adjustment back into the client size. The outer `Option` distinguishes "never
    /// applied" from an applied `None` minimum.
    window_min_inner_size: Option<Option<(i64, i64)>>,
}

fn active_item<T>(items: &[T], active: usize) -> &T {
    &items[active]
}

fn active_item_mut<T>(items: &mut [T], active: usize) -> &mut T {
    &mut items[active]
}

fn aggregate_window_minimum(
    sizes: impl IntoIterator<Item = Option<(i64, i64)>>,
) -> Option<(i64, i64)> {
    sizes
        .into_iter()
        .try_fold((1_i64, 1_i64), |(width, height), size| {
            let (next_width, next_height) = size?;
            Some((width.max(next_width), height.max(next_height)))
        })
}

fn window_minimum_changed(
    applied: &mut Option<Option<(i64, i64)>>,
    next: Option<(i64, i64)>,
) -> bool {
    if *applied == Some(next) {
        return false;
    }
    *applied = Some(next);
    true
}

fn earliest_deadline<const N: usize>(deadlines: [Option<Instant>; N]) -> Option<Instant> {
    deadlines.into_iter().flatten().min()
}

impl TabState {
    /// This tab's name, resolved through the four layers of C25.
    ///
    /// It lives on the tab and not on the runtime because every tab has one and
    /// the strip needs all of them at once; `Runtime` reaches the active tab's
    /// through `Deref`, which is how the OS window title stays the active tab's
    /// title without a second code path deciding what that is.
    fn display_title(&self) -> String {
        display_title(
            self.manual_name.as_deref(),
            self.session.window_title(),
            self.session.working_directory(),
        )
    }

    /// What this tab's tooltip says (M140).
    ///
    /// The *whole* path on the second line and not the leaf: the first line
    /// already carries the leaf whenever the folder is what named the tab, and a
    /// tip that repeated it would answer a question nobody has while leaving the
    /// one they do have — *which* `app` is this? — unanswered.
    fn tooltip_text(&self) -> String {
        let (name, source) = resolve_title(
            self.manual_name.as_deref(),
            self.session.window_title(),
            self.session.working_directory(),
        );
        let cwd = self
            .session
            .working_directory()
            .map(|path| path.to_string_lossy().into_owned());
        tooltip::tab_tip(&name, source, cwd.as_deref(), self.pinned)
    }

    /// What survives this tab being closed.
    ///
    /// One computation, because there are three doors and they must not disagree
    /// about what a tab *is*: the vault, the session file and the restore prompt
    /// all read this. The `cwd` is the shell's own last OSC 7 report and nothing
    /// else — not a guess, not a filesystem probe. A shell that never reported
    /// one seeds an empty place, and reviving that starts where a fresh tab
    /// would, which is the honest answer to "where was it?" when nobody said.
    ///
    /// The program's title is deliberately *not* in here (mock-up 4009-4010):
    /// it left with the program. Your name for the tab did not.
    fn term_leaf(&self) -> TermLeafV1 {
        TermLeafV1 {
            profile_id: profiles::PROFILES[self.profile].id.to_owned(),
            cwd: self
                .session
                .working_directory()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            manual_name: self.manual_name.clone(),
        }
    }

    /// The same three facts in the vault's spelling. A tab always seeds as a
    /// terminal; the `Files` shape exists for panes the vault also has to hold.
    fn seed(&self) -> seed::Seed {
        let leaf = self.term_leaf();
        seed::Seed::Term {
            profile_id: leaf.profile_id,
            cwd: leaf.cwd,
            manual_name: leaf.manual_name,
        }
    }
}

impl Deref for Runtime {
    type Target = TabState;

    fn deref(&self) -> &Self::Target {
        active_item(&self.tabs, self.active_tab)
    }
}

impl DerefMut for Runtime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        active_item_mut(&mut self.tabs, self.active_tab)
    }
}

impl TabState {
    /// The leaf holding the keyboard — the shell a keystroke belongs to.
    fn focused(&self) -> &LeafSession {
        self.sessions
            .get(&self.focused_leaf)
            .expect("every tab holds a session for its focused leaf")
    }

    fn focused_mut(&mut self) -> &mut LeafSession {
        self.sessions
            .get_mut(&self.focused_leaf)
            .expect("every tab holds a session for its focused leaf")
    }

    /// Every leaf of this tab, focused one included, in seat order.
    fn leaves_mut(&mut self) -> impl Iterator<Item = (&SeatId, &mut LeafSession)> {
        self.sessions.iter_mut()
    }
}

/// A tab dereferences to its focused leaf, exactly as `Runtime` dereferences to
/// its active tab.
///
/// The two hops compose: `self.session` on a `Runtime` still reads "the active
/// tab's focused shell", which is what every keystroke, every paste and every
/// hit test meant by it when a tab could only hold one. That is deliberate —
/// the alternative was rewriting a hundred and thirty call sites to say a longer
/// version of the same sentence, and each rewrite would have been a chance to
/// pick the wrong shell.
///
/// The sites this is *not* right for are the ones that meant "every shell in
/// this tab" — draining, resizing, DPI changes, reaping. Those are the loops,
/// and they are written out longhand against [`TabState::leaves_mut`] so that
/// the difference between "the focused one" and "all of them" is visible in the
/// text rather than hidden in a deref.
impl Deref for TabState {
    type Target = LeafSession;

    fn deref(&self) -> &Self::Target {
        self.focused()
    }
}

impl DerefMut for TabState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.focused_mut()
    }
}

/// A divider drag in flight. Holds only the split's identity: the geometry is
/// re-read from the current solve on every pointer move, because the answer to
/// "where is this divider" must not be a second copy of the layout.
#[derive(Clone, Copy, Debug)]
struct DividerDrag {
    split: SplitId,
    dir: Axis,
    /// The ratio this split held when the gesture began — F61's "remember what
    /// Esc has to put back", and the *whole* of what it has to put back.
    ///
    /// One `Ratio`, not a snapshot of the tree. T225 is explicit that the
    /// rollback restores that one value: a whole-tree snapshot would also undo
    /// anything else that happened while the button was down, and the things
    /// that can happen while the button is down — a command finishing and
    /// re-solving, a DPI change, the window being resized by the WM — are
    /// exactly the edits nobody asked Esc to touch.
    ///
    /// It costs nothing to be right about, because §3.3 already guarantees the
    /// restoring edit writes nothing else: `DragDivider`'s focus set is this one
    /// split, and theorem N says every ratio outside a focus set is bit-identical
    /// before and after.
    origin: bt_layout::Ratio,
}

#[derive(Clone, Copy, Debug)]
struct PendingPtyResize {
    grid: GridSize,
    physical: PhysicalSize<u32>,
    deadline: Instant,
    /// When the typed-input gate started answering "empty", or `None` if the last sample of it
    /// found content. See `sample_typed_input_gate`.
    blank_since: Option<Instant>,
}

impl PendingPtyResize {
    /// Has the gate answered "empty" for an unbroken `WINDOW_RESIZE_QUIET`, ending at `now`?
    fn blank_confirmed_at(&self, now: Instant) -> bool {
        self.blank_since
            .is_some_and(|since| now >= since + WINDOW_RESIZE_QUIET)
    }

    /// The first instant this request could be released: its own coalescing deadline, and — once a
    /// blank run has started — the end of that run's confirmation window.
    fn release_deadline(&self) -> Instant {
        self.blank_since.map_or(self.deadline, |since| {
            self.deadline.max(since + WINDOW_RESIZE_QUIET)
        })
    }
}

/// Record what the typed-input gate answered at this wake.
///
/// The gate is a question about the grid's *current* contents, and the child rewrites that grid in
/// whatever pieces a 16 KiB read happens to cut its output into. PSReadLine redraws by parking the
/// cursor on `B`, erasing, and writing the buffer back; when the erase and the rewrite arrive in
/// different reads, the wake in between finds a grid that honestly holds nothing. So a single
/// sample cannot be a release condition — only an unbroken run of them can, and any sample that
/// finds content starts the next run from scratch.
fn sample_typed_input_gate(
    pending: &mut Option<PendingPtyResize>,
    now: Instant,
    typed_input_live: bool,
) {
    let Some(pending) = pending.as_mut() else {
        return;
    };
    if typed_input_live {
        pending.blank_since = None;
    } else {
        pending.blank_since.get_or_insert(now);
    }
}

fn coalesce_pty_resize(
    pending: &mut Option<PendingPtyResize>,
    grid: GridSize,
    physical: PhysicalSize<u32>,
    observed_at: Instant,
) {
    *pending = Some(PendingPtyResize {
        grid,
        physical,
        deadline: observed_at + WINDOW_RESIZE_QUIET,
        // A new size is not a new answer about the child's buffer: a drag over an idle prompt keeps
        // sampling "empty" from its first frame on, so the confirmation window it started runs
        // alongside the coalescer's and adds nothing to the wait.
        blank_since: pending.as_ref().and_then(|pending| pending.blank_since),
    });
}

fn take_due_pty_resize(
    pending: &mut Option<PendingPtyResize>,
    now: Instant,
) -> Option<PendingPtyResize> {
    pending
        .is_some_and(|resize| now >= resize.deadline)
        .then(|| {
            pending
                .take()
                .expect("due resize was present immediately before take")
        })
}

/// The single gate between a solved grid and ConPTY.
///
/// A solve that answers what `conpty_grid` already holds must schedule nothing: the ConPTY
/// sidecar review pinned at 83dbcd3 found that any live resize call is unsafe while a shell is
/// still initializing (PSReadLine caches its own cursor anchor, and a reflow invalidates it — a
/// defect in conhost itself, not the sidecar), and a call whose columns and rows do not move is
/// never the resize that opens that window; it is only ever a spurious repeat of one already
/// applied. Startup's post-`ShowWindow` DPI reconciliation, a live OS `Resized`, and a divider drag
/// (`commit_seat_geometry`) all solve through this one point, so a clean, same-DPI session restore
/// — whose spawn-time grid already *is* the seat's resolved grid — reaches zero ConPTY resize
/// requests after spawn. Returns whether the grid actually changed, so a call site can gate its own
/// terminal-actor resize on the identical decision.
///
/// `conpty_grid` is what the child was last *told*, which is why the same answer also cancels a
/// queued request: a drag that wanders out and comes back leaves the child exactly where it already
/// was, and replaying the intermediate size at release would be a resize nobody asked for. (Before
/// the typed-input deferral the two grids could not diverge far enough for this to be observable;
/// under it they can, and "exactly one resize at release, carrying the final size" would otherwise
/// be false whenever the final size is the one the child never left.)
fn coalesce_pty_resize_on_grid_change(
    pending: &mut Option<PendingPtyResize>,
    next_grid: GridSize,
    conpty_grid: GridSize,
    physical: PhysicalSize<u32>,
    observed_at: Instant,
) -> bool {
    let changed = next_grid != conpty_grid;
    if changed {
        coalesce_pty_resize(pending, next_grid, physical, observed_at);
    } else {
        *pending = None;
    }
    changed
}

/// The retained typed-input half of the same gate (policy introduced 2026-08-04, default disabled
/// by user ruling 2026-08-06).
///
/// PSReadLine — 2.0.0 and 2.4.5 alike — reduces its cached render-anchor *column* modulo the new
/// width whenever the pane narrows, and never restores it when the pane widens again. The fault
/// arms only while its input buffer is non-empty across that narrowing; the next redraw then
/// splices into the prompt row. Three commits of ours produced byte-identical corruption and an
/// independent reference terminal reproduced it from the child's own bytes, so there is nothing on
/// our side to correct — the only faithful mitigation is not to tell the child about a width it
/// would re-anchor against while it is holding text.
///
/// When the policy feeds this machine `typed_input_live = true`, a due resize is held back and
/// released once that has been false for an unbroken `WINDOW_RESIZE_QUIET`. What releases is still
/// a *question about current state* and
/// never a timer over the drag: the input region closing (submission), its content going empty (the
/// line cleared), a screen switch, a reset, ED 2 or an alternate-screen transition all make
/// `DualPlaneSession::typed_shell_input_live` answer `false`, and every one of them reaches us as
/// child output — which wakes the event loop on its own. A buffer that still holds text is held for
/// as long as it holds it, however long the drag has been over.
///
/// The confirmation window is what a single sample cannot give: the gate reads the grid, and a
/// redraw split across two reads leaves the grid momentarily empty between them
/// (`sample_typed_input_gate`). Waiting out the same quiet period the coalescer already uses costs
/// an idle prompt nothing — its blank run starts at the drag's first frame — and costs a line that
/// was genuinely emptied one quiet window.
fn release_due_pty_resize(
    pending: &mut Option<PendingPtyResize>,
    now: Instant,
    typed_input_live: bool,
) -> Option<PendingPtyResize> {
    sample_typed_input_gate(pending, now, typed_input_live);
    pending
        .is_some_and(|pending| pending.blank_confirmed_at(now))
        .then(|| take_due_pty_resize(pending, now))
        .flatten()
}

/// The whole scheduling decision `Runtime::schedule_grid_change` makes, with no window in it:
/// queue (or cancel) what the child still owes, and answer the grid our own actor should reflow to
/// *now* — `None` while the retained gate holds. With the policy off, `typed_input_live` reaches
/// this function as false and the actor always follows the solved grid immediately.
fn plan_grid_change(
    pending: &mut Option<PendingPtyResize>,
    next_grid: GridSize,
    conpty_grid: GridSize,
    local_grid: GridSize,
    physical: PhysicalSize<u32>,
    observed_at: Instant,
    typed_input_live: bool,
) -> Option<GridSize> {
    coalesce_pty_resize_on_grid_change(pending, next_grid, conpty_grid, physical, observed_at);
    // A pointer frame is a sample of the gate like any other, and taking it here is what keeps the
    // confirmation window free for the case it must stay free for: a drag over an idle prompt has
    // been reading "empty" since its first frame, so the window has long since elapsed by the time
    // the drag stops and the coalescer's own deadline arrives.
    sample_typed_input_gate(pending, observed_at, typed_input_live);
    (next_grid != local_grid && !typed_input_live).then_some(next_grid)
}

/// The instant the coalesced ConPTY resize could next be released, or `None` while nothing is owed
/// or the typed-input gate is holding it.
///
/// Withholding it while held is not an optimisation: the deadline has usually already passed by
/// then, and handing an event loop a past instant to wait until is a spin. There is nothing to
/// wait *for* either — a gate that is holding on *content* is released by child output, which wakes
/// the loop by itself. A gate reading empty is the opposite case: the confirmation window it is
/// serving may well end with no further output at all, so the loop is given that instant to wake
/// at.
fn pty_resize_wake_deadline(
    pending: Option<PendingPtyResize>,
    typed_input_live: bool,
    now: Instant,
) -> Option<Instant> {
    (!typed_input_live)
        .then_some(pending)
        .flatten()
        .map(|pending| pending.release_deadline())
        // `about_to_wait` has already serviced everything due at `now`. Re-offering that instant
        // (or an older coalescing deadline retained across a gate transition) makes winit wake
        // immediately and re-enter this turn without an external event. Only a future confirmation
        // boundary is a useful wake-up request.
        .filter(|deadline| *deadline > now)
}

/// Advance the resize gate and derive its next wake from the same gate sample.
///
/// Keeping these two decisions together prevents `about_to_wait` from servicing a content-holding
/// state and then deriving `WaitUntil` from a different, empty-state reading of the session. The
/// latter can expose the request's already-due coalescing deadline while `blank_since` still says
/// that no empty confirmation run has begun.
fn service_pending_pty_resize(
    pending: &mut Option<PendingPtyResize>,
    now: Instant,
    typed_input_live: bool,
) -> (Option<PendingPtyResize>, Option<Instant>) {
    let due = release_due_pty_resize(pending, now, typed_input_live);
    let wake = pty_resize_wake_deadline(*pending, typed_input_live, now);
    (due, wake)
}

const fn typed_input_resize_deferral_active(
    policy: bool,
    pty_present: bool,
    typed_input_live: bool,
) -> bool {
    policy && pty_present && typed_input_live
}

/// The private shell-integration input owed after one successful ConPTY resize commit.
///
/// `false` includes every session that has never emitted OSC 133, every closed input region, and
/// every alternate screen. Returning `None` is what makes those cases a byte-for-byte no-op rather
/// than a best-effort guess about which shell might be present.
fn psreadline_resize_repaint_input(shell_input_region_open: bool) -> Option<&'static [u8]> {
    shell_input_region_open.then_some(PSREADLINE_INVOKE_PROMPT_INPUT)
}

fn replace_psreadline_resize_reanchor_debt(pending: &mut bool, shell_input_region_open: bool) {
    *pending = psreadline_resize_repaint_input(shell_input_region_open).is_some();
}

fn take_psreadline_resize_reanchor_input(
    pending: &mut bool,
    shell_input_region_open: bool,
) -> Option<&'static [u8]> {
    std::mem::take(pending)
        .then(|| psreadline_resize_repaint_input(shell_input_region_open))
        .flatten()
}

#[derive(Clone)]
enum MouseRoute {
    Local(SelectionDrag),
    Forward(input::MouseProtocolButton),
    MathBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UserInputKind {
    Keyboard,
    Mouse,
}

impl UserInputKind {
    fn returns_view_to_live(self) -> bool {
        matches!(self, Self::Keyboard)
    }
}

fn live_viewport_mouse_hit(frame: &ViewportFrame, hit: bt_render::GridHit) -> bt_render::GridHit {
    bt_render::GridHit {
        // The mapping belongs to the actual presented frame. Frozen/staging pixels above the
        // mutable viewport retain Windows Terminal's clamp-to-live-row-zero behaviour.
        row: frame
            .live_point_at(hit.row, hit.column)
            .map_or(0, |point| point.row),
        column: hit.column,
    }
}

fn route_forwarded_mouse_button(
    route: &mut Option<MouseRoute>,
    state: ElementState,
    button: input::MouseProtocolButton,
    hit: bt_render::GridHit,
    modes: TerminalModes,
    modifiers: ModifiersState,
) -> Option<Vec<u8>> {
    let forward = !modifiers.shift_key() && modes.mouse_tracking != MouseTracking::Off;
    match state {
        ElementState::Pressed if forward => {
            *route = Some(MouseRoute::Forward(button));
            Some(input::mouse_bytes(
                modes.sgr_mouse,
                button,
                input::MouseProtocolEvent::Press,
                hit.row,
                hit.column,
                modifiers,
            ))
        }
        ElementState::Released if matches!(route, Some(MouseRoute::Forward(_))) => {
            *route = None;
            Some(input::mouse_bytes(
                modes.sgr_mouse,
                button,
                input::MouseProtocolEvent::Release,
                hit.row,
                hit.column,
                modifiers,
            ))
        }
        _ => None,
    }
}

#[derive(Clone)]
struct SelectionDrag {
    mode: SelectionDragMode,
    origin_row: u32,
    origin_column: u32,
    origin: ViewSelection,
    hyperlink: Option<HyperlinkHit>,
    open_hyperlink_on_release: bool,
    local_image_activation: LocalImageActivation,
}

#[derive(Clone, Copy)]
enum SelectionDragMode {
    Linear,
    Word,
    Line,
}

fn should_copy_on_select_release(route: Option<&MouseRoute>, single_click: bool) -> bool {
    !single_click && matches!(route, Some(MouseRoute::Local(_)))
}

#[derive(Default)]
struct ClickTracker {
    last_at: Option<Instant>,
    last_cell: Option<(u32, u32)>,
    count: u8,
}

impl ClickTracker {
    fn register(&mut self, row: u32, column: u32, now: Instant) -> u8 {
        if self.last_cell == Some((row, column))
            && self
                .last_at
                .is_some_and(|last| now.saturating_duration_since(last) <= MULTI_CLICK_INTERVAL)
        {
            self.count = self.count % 3 + 1;
        } else {
            self.count = 1;
        }
        self.last_at = Some(now);
        self.last_cell = Some((row, column));
        self.count
    }
}

/// How long a press on a tab holds its activation back (mock-up 5756-5762).
///
/// "Edge parity with a grace period (user rulings 2026-07-18, two passes): a
/// left PRESS chooses the tab, but the switch lands ~180ms later — a quick
/// drag-out to split never flashes the pressed tab's content over the layout you
/// are aiming at."
const TAB_PRESS_ACTIVATION_GRACE: Duration = Duration::from_millis(180);

/// How far the pointer may travel before a press stops being a press
/// (`startDrag`'s own `Math.hypot(...) < 6`, mock-up 6755).
///
/// Logical pixels, because it is a distance the *hand* travels: the same gesture
/// on a 200% display covers twice the physical pixels and is still the same
/// gesture.
///
/// J113 — one number for every source. The mock-up has a single `startDrag` and
/// therefore a single threshold, and the reason is not economy: the 6px is the
/// hand's own tolerance for holding still, and a pane that needed more resolve to
/// pick up than a tab would be reporting something about *panes* that is really
/// about fingers.
const DRAG_THRESHOLD_LOGICAL_PX: f64 = 6.0;

/// The 6px, and whether it has been crossed yet — the one thing every press that
/// can become a drag has in common (J112/J113).
///
/// It is kept apart from everything else a press holds, because everything else a
/// press holds is source-specific: a tab press owes its tab an activation
/// ([`TabPressPromise`]), a pane press owes nothing at all, and both cross the
/// same six pixels in the same way. Extracting it is what makes "the same
/// threshold" a fact about the code rather than a promise in a comment.
#[derive(Clone, Copy, Debug)]
struct DragLatch {
    /// Where the press landed, in physical pixels.
    origin: PhysicalPosition<f64>,
    /// Whether the pointer has already left the press's own neighbourhood, so
    /// the gesture that starts there has started.
    begun: bool,
}

impl DragLatch {
    fn new(origin: PhysicalPosition<f64>) -> Self {
        Self {
            origin,
            begun: false,
        }
    }

    /// Tell the latch where the pointer is now. Returns whether this is the move
    /// the drag begins on — once per press, and for every press.
    fn travelled(&mut self, position: PhysicalPosition<f64>, scale: f64) -> bool {
        if self.begun {
            return false;
        }
        let threshold = DRAG_THRESHOLD_LOGICAL_PX * scale;
        if (position.x - self.origin.x).hypot(position.y - self.origin.y) < threshold {
            return false;
        }
        self.begun = true;
        true
    }
}

/// What a press on a tab still owes it.
///
/// The press-activation contract is a promise with three possible states, and
/// naming them is the whole of what makes T5 able to hang J106/J108 on this: a
/// drag that starts has to be able to ask "was the switch already paid?" and a
/// drag that is cancelled has to be able to pay it late.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabPressPromise {
    /// Chosen but not yet shown: the grace period is still running and the
    /// pointer has not travelled.
    Pending,
    /// The pointer left the press's own neighbourhood before the grace period
    /// ran out, so the delayed activation was abandoned. Nothing has been
    /// shown, which is the point: "a quick drag-out to split never flashes the
    /// pressed tab's content". It is the mock-up's `drag.started` seen from the
    /// promise's side only — whether a drag has begun is
    /// [`TabPress::drag_begun`], because a press that owed nothing to begin
    /// with can drag without ever passing through here.
    Slipped,
    /// The switch has landed. `drag.pressActivated` in the mock-up.
    Paid,
}

/// A left press being held on a tab.
///
/// Keyed on [`TabId`] rather than on a strip index, because a press outlives
/// reorders: pinning a tab from a menu, or a background tab closing, renumbers
/// the strip under a finger that has not moved.
#[derive(Clone, Copy, Debug)]
struct TabPress {
    tab: TabId,
    /// The six pixels, shared with every other press in the window (J113).
    ///
    /// Held apart from [`Self::promise`] on purpose, because the two answer
    /// different questions: the promise is what the press still *owes* its tab,
    /// and the latch is whether the hand has begun carrying it. A press on the
    /// tab you are already looking at owes nothing the instant it lands, and it
    /// is exactly as draggable as any other — reading the debt to decide the
    /// gesture is what made the active tab immovable.
    latch: DragLatch,
    /// When the delayed activation is due.
    deadline: Instant,
    promise: TabPressPromise,
}

impl TabPress {
    /// A press that owes the tab an activation.
    fn armed(tab: TabId, origin: PhysicalPosition<f64>, now: Instant) -> Self {
        Self {
            tab,
            latch: DragLatch::new(origin),
            deadline: now + TAB_PRESS_ACTIVATION_GRACE,
            promise: TabPressPromise::Pending,
        }
    }

    /// A press onto the tab that is *already* showing, which owes it nothing.
    ///
    /// The mock-up arms no timer here at all (`wsId !== state.active`, 5755),
    /// and the reason is not efficiency: an activation that has nothing to
    /// change must not be able to *become* one later, because by then the
    /// active tab may be somebody else.
    ///
    /// Owing nothing is not the same as doing nothing. This press still holds a
    /// tab, and hands carry the tab they are looking at more often than any
    /// other — see [`Self::travelled`].
    fn settled(tab: TabId, origin: PhysicalPosition<f64>, now: Instant) -> Self {
        Self {
            promise: TabPressPromise::Paid,
            ..Self::armed(tab, origin, now)
        }
    }

    /// Tell the press where the pointer is now. Returns whether this is the
    /// move the drag begins on — once per press, and for every press.
    ///
    /// Crossing the threshold *also* withdraws a delayed activation that is
    /// still waiting, and that is a side effect rather than the answer. Only a
    /// `Pending` press can slip; a promise already paid stays paid, because
    /// travelling after the switch has landed is a drag of a tab you are
    /// already looking at and taking the view back would be a second,
    /// unasked-for switch. What it is not is a reason to refuse the drag: a
    /// press that owes its tab nothing — the one onto the active tab, or one
    /// whose grace period ran out under a still-held finger — is a hand on a
    /// tab like any other.
    fn travelled(&mut self, position: PhysicalPosition<f64>, scale: f64) -> bool {
        if !self.latch.travelled(position, scale) {
            return false;
        }
        if self.promise == TabPressPromise::Pending {
            self.promise = TabPressPromise::Slipped;
        }
        true
    }

    /// Whether the grace period has run out on a press that is still owed.
    /// Returns true exactly once — the caller activates, and the promise is paid.
    fn matured(&mut self, now: Instant) -> bool {
        if self.promise != TabPressPromise::Pending || now < self.deadline {
            return false;
        }
        self.promise = TabPressPromise::Paid;
        true
    }

    /// Whether letting go here pays the promise, given the tab the pointer is
    /// over as it lifts.
    ///
    /// **Ruling** (the mock-up states this only through browser mechanics).
    /// Release activates exactly when the pointer lifts on the tab it pressed —
    /// which is the DOM `click` contract the mock-up's `selectTab` handler
    /// (5735) actually runs on: `click` fires on the nearest common ancestor of
    /// the press and the release, so a release on the same tab is a click on
    /// that tab and a release anywhere else is not. Stated in geometry rather
    /// than in drag state, it needs no drag machinery to be true, and it says
    /// both of the things the ticket asks for at once: a quick click is a click
    /// (J105), and letting go somewhere else leaves the promise unpaid for T5's
    /// drop to answer (J108).
    fn released_over(&mut self, tab: Option<TabId>) -> bool {
        if self.promise == TabPressPromise::Paid || tab != Some(self.tab) {
            return false;
        }
        self.promise = TabPressPromise::Paid;
        true
    }

    /// When the event loop must wake to pay this promise, if it still owes one.
    fn wake_deadline(&self) -> Option<Instant> {
        (self.promise == TabPressPromise::Pending).then_some(self.deadline)
    }
}

/// What a press on a tab turned out to be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabClick {
    Single,
    Double,
}

/// The strip's own click counter — [`ClickTracker`] for tabs.
///
/// It counts to two and no further: the tab strip has one multi-click verb and
/// the terminal's word/line/paragraph ladder has no counterpart here.
///
/// Identity is the tab, not a pixel neighbourhood, and that is the faithful
/// reading rather than a shortcut: `dblclick` fires on the element both clicks
/// share, so "the same tab" *is* the browser's own slop test. It also survives
/// the one thing a pixel test would get wrong — a strip that scrolled between
/// the two clicks, where the same tab is at a different address.
#[derive(Default)]
struct TabClicks {
    last: Option<(TabId, Instant)>,
}

impl TabClicks {
    fn register(&mut self, tab: TabId, now: Instant) -> TabClick {
        let paired = self.last.is_some_and(|(last_tab, last_at)| {
            last_tab == tab && now.saturating_duration_since(last_at) <= MULTI_CLICK_INTERVAL
        });
        // A double click consumes its own history. Without this a third press
        // inside the window would pair with the second and open the editor a
        // second time; the browser resets the same way (`detail` restarts).
        self.last = (!paired).then_some((tab, now));
        if paired {
            TabClick::Double
        } else {
            TabClick::Single
        }
    }

    /// Forget the last click. Anything that is not a plain press on a tab body
    /// breaks the chain — the `×`, the pin, a middle click, a press on another
    /// piece of chrome — because none of them is the first half of a double
    /// click on a tab (J99: "a double click on `.close` is two button presses").
    fn interrupt(&mut self) {
        self.last = None;
    }
}

/// What a key did to the tab-name editor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenameVerdict {
    /// The editor took the key and is still open.
    Held,
    /// Enter: write the draft through (mock-up 5895).
    Commit,
    /// Escape: leave the name as it was (mock-up 5896).
    Cancel,
}

/// The tab-name editor — the tab itself, in edit mode.
///
/// "The whole editor falls out of one fact: `name` is an override. So the box
/// starts holding YOUR name only — never the auto one — and the placeholder
/// shows what is underneath" (mock-up 5840-5845). Both halves of that are here:
/// [`Self::open`] seeds from the manual name alone, and an empty draft commits
/// as `None` rather than as `""`.
///
/// Keyed on [`TabId`] for the same reason [`TabPress`] is: the editor must not
/// follow a strip position across a reorder.
///
/// The caret is a byte offset and every edit moves it on a `char` boundary. That
/// is the minimum a name field needs and is deliberately not grapheme-aware:
/// splitting a combining sequence is the one thing this loses, and buying the
/// segmentation for a forty-character label is not a trade this slice makes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct TabRename {
    tab: TabId,
    text: String,
    /// The caret, as a byte offset into `text`.
    caret: usize,
    /// Whether the whole draft is selected — the one selection this editor has
    /// (`input.select()`, mock-up 5870). It is a flag rather than an anchor
    /// because there is no gesture in this slice that can make any *other*
    /// selection: the mock-up's own editor gets exactly this one, at open, and
    /// the first thing you type replaces it.
    select_all: bool,
    /// The first character actually drawn, as a byte offset — how a box narrower
    /// than its text keeps the caret in sight. Owned by the editor and moved by
    /// the renderer's measurements, because only the font knows when the caret
    /// has walked off the end.
    first_visible: usize,
}

impl TabRename {
    /// Open on a tab, seeded from its manual name.
    ///
    /// "初值 = 现有 `name`（只放你的名字）" — the auto name is never put in the
    /// box, it is put *behind* it as the placeholder, which is what tells you
    /// what clearing the box will get you.
    fn open(tab: TabId, manual_name: Option<&str>) -> Self {
        let text = manual_name.unwrap_or_default().to_owned();
        Self {
            tab,
            // `input.focus(); input.select()` (5869-5870): the caret sits at the
            // end of the selection, which is the end of the text.
            caret: text.len(),
            select_all: !text.is_empty(),
            first_visible: 0,
            text,
        }
    }

    /// Drop the selection, collapsing to `caret`, and report whether there was
    /// one. Every editing and navigation verb starts here.
    fn collapse(&mut self) -> bool {
        std::mem::take(&mut self.select_all)
    }

    /// Replace the selection (or insert at the caret) with `text`.
    ///
    /// The one door for typed characters, for an IME commit and for anything
    /// else that arrives as text, so "typing over a fresh selection replaces it"
    /// is true once rather than once per source.
    fn insert(&mut self, text: &str) {
        if self.collapse() {
            self.text.clear();
            self.caret = 0;
        }
        // Control characters never reach a tab name. `clean_title` strips them
        // on the way out too, but letting one *into* the draft would put a caret
        // on the far side of something that draws as nothing.
        let text = text
            .chars()
            .filter(|character| !matches!(character, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}'))
            .collect::<String>();
        if text.is_empty() {
            return;
        }
        self.text.insert_str(self.caret, &text);
        self.caret += text.len();
        self.first_visible = self.first_visible.min(self.caret);
    }

    /// Backspace: the selection if there is one, else the character before the
    /// caret.
    fn backspace(&mut self) {
        if self.collapse() {
            self.text.clear();
            self.caret = 0;
        } else if let Some((start, _)) = self.text[..self.caret].char_indices().next_back() {
            self.text.replace_range(start..self.caret, "");
            self.caret = start;
        }
        self.clamp_scroll();
    }

    /// Delete: the selection if there is one, else the character after the
    /// caret.
    fn delete(&mut self) {
        if self.collapse() {
            self.text.clear();
            self.caret = 0;
        } else if let Some(character) = self.text[self.caret..].chars().next() {
            let end = self.caret + character.len_utf8();
            self.text.replace_range(self.caret..end, "");
        }
        self.clamp_scroll();
    }

    /// ← : to the start of the selection if there is one, else back one
    /// character. Collapsing to the *near* edge is what every text field does
    /// and is why an accidental select-all is not destructive.
    fn move_left(&mut self) {
        if self.collapse() {
            self.caret = 0;
        } else if let Some((start, _)) = self.text[..self.caret].char_indices().next_back() {
            self.caret = start;
        }
        self.clamp_scroll();
    }

    /// → : to the end of the selection if there is one, else on one character.
    fn move_right(&mut self) {
        if self.collapse() {
            self.caret = self.text.len();
        } else if let Some(character) = self.text[self.caret..].chars().next() {
            self.caret += character.len_utf8();
        }
    }

    fn move_home(&mut self) {
        self.collapse();
        self.caret = 0;
        self.first_visible = 0;
    }

    fn move_end(&mut self) {
        self.collapse();
        self.caret = self.text.len();
    }

    /// Keep the drawn window from starting after the caret, which is the one
    /// way an edit alone (rather than a measurement) can invalidate it.
    fn clamp_scroll(&mut self) {
        self.first_visible = self.first_visible.min(self.caret).min(self.text.len());
        while self.first_visible > 0 && !self.text.is_char_boundary(self.first_visible) {
            self.first_visible -= 1;
        }
    }

    /// What committing this draft writes into `manual_name`.
    ///
    /// "空串 = 撤销 override" — `s.name = v || null` (mock-up 5883). The
    /// sanitiser runs first and can *produce* the empty string from a draft that
    /// was only spaces, and that is the same answer: a name of nothing is not a
    /// name, it is the absence of one, and the placeholder already showed you
    /// what you would land on.
    fn committed_name(&self) -> Option<String> {
        let value = clean_title(&self.text);
        (!value.is_empty()).then_some(value)
    }
}

/// Route one key press to the open tab-name editor (mock-up 5893-5897).
///
/// Every key returns a verdict, and there is deliberately no "not mine" arm:
/// while the editor is open it owns the keyboard entire (J103, and
/// `docs/DESIGN.md` §7.1.5's `InputOwner = Rename`). A key it has no verb for is
/// swallowed rather than passed down, because the thing underneath is a terminal
/// and the alternative is typing your tab's name into a shell.
fn rename_key(editor: &mut TabRename, key: &Key, modifiers: ModifiersState) -> RenameVerdict {
    // A chord is not text. Ctrl/Alt-modified keys are swallowed unhandled: this
    // editor has no clipboard and no word verbs, and letting the chord through
    // to the terminal is exactly what §7.1.5 forbids.
    let chorded = modifiers.control_key() || modifiers.alt_key() || modifiers.super_key();
    match key {
        Key::Named(NamedKey::Enter) => return RenameVerdict::Commit,
        Key::Named(NamedKey::Escape) => return RenameVerdict::Cancel,
        _ if chorded => {}
        Key::Named(NamedKey::Backspace) => editor.backspace(),
        Key::Named(NamedKey::Delete) => editor.delete(),
        Key::Named(NamedKey::ArrowLeft) => editor.move_left(),
        Key::Named(NamedKey::ArrowRight) => editor.move_right(),
        Key::Named(NamedKey::Home) => editor.move_home(),
        Key::Named(NamedKey::End) => editor.move_end(),
        // `Space` is the one printable character winit reports as a named key,
        // so it needs the same door the characters use rather than a verb.
        Key::Named(NamedKey::Space) => editor.insert(" "),
        Key::Character(text) => editor.insert(text),
        _ => {}
    }
    RenameVerdict::Held
}

#[derive(Default)]
struct HyperlinkHover {
    candidate: Option<HyperlinkHit>,
    show_at: Option<Instant>,
    active: Option<HyperlinkHit>,
    blocked: bool,
}

impl HyperlinkHover {
    fn observe(&mut self, hit: Option<HyperlinkHit>, now: Instant) -> bool {
        if self.active.is_some() && hit.as_ref() == self.active.as_ref() {
            self.candidate = None;
            self.show_at = None;
            return false;
        }
        if self.candidate.is_some() && hit.as_ref() == self.candidate.as_ref() {
            return false;
        }
        let active_changed = self.active.take().is_some();
        self.blocked = false;
        // The underline is the affordance and follows the candidate immediately; only the status
        // tooltip waits out the hover delay. A candidate change therefore needs a republish too.
        let candidate_changed = self.candidate.is_some() || hit.is_some();
        self.candidate = hit;
        self.show_at = self.candidate.as_ref().map(|_| now + HYPERLINK_HOVER_DELAY);
        active_changed || candidate_changed
    }

    /// The link whose span should render underlined right now: the settled hover if the tooltip is
    /// up, otherwise the instant candidate under the pointer.
    fn underline_target(&self) -> Option<&HyperlinkHit> {
        self.active.as_ref().or(self.candidate.as_ref())
    }

    fn activate_if_due(&mut self, now: Instant) -> bool {
        if self.show_at.is_none_or(|deadline| now < deadline) {
            return false;
        }
        self.show_at = None;
        self.active = self.candidate.take();
        self.blocked = false;
        self.active.is_some()
    }

    fn show_blocked(&mut self, hyperlink: HyperlinkHit) {
        self.candidate = None;
        self.show_at = None;
        self.active = Some(hyperlink);
        self.blocked = true;
    }

    fn clear(&mut self) -> bool {
        self.candidate = None;
        self.show_at = None;
        self.blocked = false;
        self.active.take().is_some()
    }

    fn status_text(&self, columns: usize) -> Option<String> {
        let uri = self
            .active
            .as_ref()?
            .uri
            .chars()
            .map(|character| {
                if character.is_control() {
                    '\u{fffd}'
                } else {
                    character
                }
            })
            .collect::<Vec<_>>();
        if columns == 0 {
            return None;
        }
        let suffix = if self.blocked { " · blocked" } else { "" };
        let suffix_len = suffix.chars().count();
        if columns <= suffix_len {
            return Some("blocked".chars().take(columns).collect());
        }
        let target_columns = columns - suffix_len;
        let mut status = if uri.len() > target_columns {
            uri.into_iter()
                .take(target_columns.saturating_sub(1))
                .chain(['…'])
                .collect::<String>()
        } else {
            uri.into_iter().collect()
        };
        status.push_str(suffix);
        Some(status)
    }
}

/// What a settled hover is a hover *of*. Two shapes reach the flyout, and they differ in exactly
/// one thing: whether a cache miss can be filled by reading a file.
///
/// A printed path, a `file://` URI and an OSC 8 link target all name a file on disk — the worker
/// reads it. An OSC 1337 payload names nothing: its bytes arrived once in the stream and were
/// decoded then, so the flyout can only show what was already remembered under the decoder's
/// content key. Both shapes are one identity string in `peek_cache`, which is why the rest of the
/// pipeline never has to ask which is which.
#[derive(Clone, Debug)]
struct PeekSubject {
    /// Identity in `peek_cache`: a normalized path for a named file, the decoder's content key for
    /// a stream payload. Two observations are the same hover exactly when these agree.
    key: String,
    /// The file to read on a cache miss, when there is one.
    path: Option<PathBuf>,
}

/// Identity is the key alone — the path beside it is only how a miss is filled. A file the terminal
/// printed twice under two spellings is one picture, and sliding between them must not restart the
/// settle clock or re-decode anything.
impl PartialEq for PeekSubject {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for PeekSubject {}

impl PeekSubject {
    fn from_path(path: PathBuf) -> Self {
        Self {
            key: normalized_local_image_path_key(&path),
            path: Some(path),
        }
    }

    /// A payload the session already decoded, addressed by the key it was cached under.
    fn from_content_key(key: String) -> Self {
        Self { key, path: None }
    }
}

#[derive(Clone, Debug)]
struct PeekCandidate {
    subject: PeekSubject,
    /// Physical pointer position of the most recent observation; the flyout anchors where the
    /// hover settles, not where it began.
    pointer: PhysicalPosition<f64>,
}

/// Hover state for the local-image peek flyout (preview matrix §4): same 300ms settle clock as
/// the hyperlink tooltip, keyed by path so sliding along one path span never restarts the timer.
#[derive(Default)]
struct PeekHover {
    candidate: Option<PeekCandidate>,
    show_at: Option<Instant>,
    /// The settled hover whose flyout is showing (or whose decode is in flight). The settle
    /// pointer is kept so a decode that finishes later still places the flyout where the hover
    /// settled.
    active: Option<PeekCandidate>,
}

impl PeekHover {
    /// Track the path under the pointer. Returns true when a previously shown flyout must be
    /// hidden because the pointer left its path span.
    fn observe(
        &mut self,
        subject: Option<PeekSubject>,
        pointer: PhysicalPosition<f64>,
        now: Instant,
    ) -> bool {
        if self.active.is_some()
            && subject.as_ref() == self.active.as_ref().map(|active| &active.subject)
        {
            self.candidate = None;
            self.show_at = None;
            return false;
        }
        let active_hidden = self.active.take().is_some();
        match subject {
            Some(subject) => {
                let same_span =
                    self.candidate.as_ref().map(|candidate| &candidate.subject) == Some(&subject);
                self.candidate = Some(PeekCandidate { subject, pointer });
                if !same_span {
                    self.show_at = Some(now + HYPERLINK_HOVER_DELAY);
                }
            }
            None => {
                self.candidate = None;
                self.show_at = None;
            }
        }
        active_hidden
    }

    /// The candidate whose settle deadline has passed, promoted to active. The caller resolves
    /// the thumbnail (cache or worker) and shows the flyout.
    fn activate_if_due(&mut self, now: Instant) -> Option<PeekCandidate> {
        if self.show_at.is_none_or(|deadline| now < deadline) {
            return None;
        }
        self.show_at = None;
        let candidate = self.candidate.take()?;
        self.active = Some(candidate.clone());
        Some(candidate)
    }

    /// Drop all peek state. Returns true when a visible flyout must be hidden.
    fn clear(&mut self) -> bool {
        self.candidate = None;
        self.show_at = None;
        self.active.take().is_some()
    }
}

/// App-side memory of peek decode outcomes, keyed by `PeekSubject::key` — the decoder's normalized
/// path identity for a named file, its content key for a stream payload.
/// `Failed` entries keep a missing or corrupt file from re-hitting the disk on every hover;
/// the payload bytes are shared `Arc`s with the worker decoder's own cache.
///
/// `Ready` holds the *native* decode. It is CPU memory the worker's decoder already retains, and
/// it is what a resample at any later display size is computed from — it never reaches the GPU.
enum PeekCacheEntry {
    Pending,
    Failed,
    Ready {
        key: String,
        rgba: Arc<[u8]>,
        width_px: u32,
        height_px: u32,
    },
}

/// One content at one display size: `(content key, display width, display height)`.
type PeekThumbnailTarget = (String, u32, u32);

/// The display-sized raster the flyout hands the renderer — the only peek pixels that ever reach
/// the GPU.
///
/// Cache policy: exactly one entry. A peek is transient and singular (one flyout at a time), the
/// display box depends on the viewport at hover time, and the native decodes stay path-keyed in
/// `peek_cache`, so re-showing at a size this entry does not hold costs one worker resample and
/// never a disk read. `key` carries the display size (`display_texture_key`), so the shared GPU
/// LRU can never serve a raster rastered for a different box.
struct PeekThumbnail {
    /// Identity of the native decode this was resampled from, matched against `PeekCacheEntry`.
    content_key: String,
    key: String,
    rgba: Arc<[u8]>,
    width_px: u32,
    height_px: u32,
}

/// Persistent preview-seat state. Native pixels remain in `peek_cache`; this holds only the one
/// display-sized raster and the question currently in flight for the solver's body rectangle.
struct PreviewImageState {
    path: PathBuf,
    pending: Option<PeekThumbnailTarget>,
    raster: Option<PeekThumbnail>,
    failure: Option<String>,
    /// The decode's native dimensions, once known — shown beside the file name so the title
    /// answers "how big is this really" while the body shows the fitted version.
    native: Option<(u32, u32)>,
    /// The shared resize quiet boundary. Geometry follows every pointer event, but the expensive
    /// exact-size resample is asked only after this instant lands without another resize.
    resize_scale_deadline: Option<Instant>,
}

impl PreviewImageState {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            pending: None,
            raster: None,
            failure: None,
            native: None,
            resize_scale_deadline: None,
        }
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }

    fn title(&self) -> String {
        let name = self.file_name();
        match self.native {
            Some((width, height)) => format!("{name} \u{2014} {width}\u{00d7}{height}"),
            None => name,
        }
    }

    fn message(&self) -> Option<String> {
        self.failure.clone().or_else(|| {
            (self.pending.is_some() || self.raster.is_none())
                .then(|| format!("Loading {}\u{2026}", self.file_name()))
        })
    }

    /// Accept only the answer to the newest size question. A superseded answer leaves `pending`
    /// intact, so the chrome cannot briefly claim success and then remain stuck without a raster.
    fn accept_scaled(&mut self, scaled: bt_term::ScaledInlineImage) -> bool {
        let delivered: PeekThumbnailTarget = (
            scaled.content_key.clone(),
            scaled.width_px,
            scaled.height_px,
        );
        if self.pending.as_ref() != Some(&delivered) {
            return false;
        }
        self.pending = None;
        self.raster = Some(PeekThumbnail::from_scaled(scaled));
        self.failure = None;
        true
    }

    fn defer_resize_scale(&mut self, observed_at: Instant) {
        self.resize_scale_deadline = Some(observed_at + WINDOW_RESIZE_QUIET);
    }

    fn finish_resize_scale_if_quiet(&mut self, now: Instant) -> bool {
        if self
            .resize_scale_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.resize_scale_deadline = None;
            true
        } else {
            false
        }
    }
}

impl PeekThumbnail {
    fn from_scaled(scaled: bt_term::ScaledInlineImage) -> Self {
        Self {
            content_key: scaled.content_key,
            key: scaled.key,
            rgba: scaled.rgba,
            width_px: scaled.width_px,
            height_px: scaled.height_px,
        }
    }

    fn matches(&self, target: &PeekThumbnailTarget) -> bool {
        self.content_key == target.0 && (self.width_px, self.height_px) == (target.1, target.2)
    }

    /// The overlay the renderer draws: display-sized pixels under a display-sized identity, and a
    /// pointer anchor. This is the only path by which peek pixels reach the GPU.
    fn overlay(&self, pointer: PhysicalPosition<f64>) -> PeekImageOverlay {
        PeekImageOverlay {
            key: self.key.clone(),
            rgba: Arc::clone(&self.rgba),
            width_px: self.width_px,
            height_px: self.height_px,
            pointer_x: pointer.x as f32,
            pointer_y: pointer.y as f32,
        }
    }
}

/// The one resample that stands between a native decode and the flyout, addressed to the worker.
fn peek_scale_task(
    target: &PeekThumbnailTarget,
    rgba: Arc<[u8]>,
    native_width_px: u32,
    native_height_px: u32,
) -> bt_term::InlineImageScaleTask {
    let (content_key, display_width_px, display_height_px) = target;
    bt_term::InlineImageScaleTask {
        // The peek is not an occurrence in the document; its identity is the content key.
        occurrence_id: 0,
        content_key: content_key.clone(),
        rgba,
        width_px: native_width_px,
        height_px: native_height_px,
        display_width_px: *display_width_px,
        display_height_px: *display_height_px,
    }
}

/// The `peek_cache` identity a decoration-worker decode must be remembered under, so that the
/// hover which later asks for the same picture finds it instead of asking the disk again.
///
/// It is `PeekSubject`'s own rule, stated once: a named file is its normalized path, a stream
/// payload is the decoder's content key. Deriving it here rather than at the two call sites is what
/// makes "verification warms the peek" a fact about one key rather than a coincidence between two.
fn peek_cache_key_for_decode(
    source: &bt_term::InlineImageSource,
    decoded: &bt_term::DecodedInlineImage,
) -> String {
    match source {
        bt_term::InlineImageSource::Osc1337(_) => decoded.key.clone(),
        bt_term::InlineImageSource::LocalPath(path) => normalized_local_image_path_key(path),
    }
}

/// One frame's image references, with the row stride they were scanned at.
///
/// The stride travels with the list because a `GridHit` is a row and a column while a reference is
/// a set of cell indices; keeping the frame's own column count beside the scan is what makes the
/// two the same coordinate rather than two coordinates that usually agree.
///
/// One hovered cell resolves to at most one reference: the first that covers it. The scan pushes
/// printed text before link targets, so where a cell carries both — a link whose label spells the
/// path — the answer is what the pointer is actually standing on. Both name the same file anyway.
#[derive(Default)]
struct FrameImageReferences {
    columns: u32,
    references: Vec<bt_term::FrameImageReference>,
}

impl FrameImageReferences {
    fn at(&self, hit: bt_render::GridHit) -> Option<&bt_term::FrameImageReference> {
        let cell = hit.row.checked_mul(self.columns)?.checked_add(hit.column)?;
        self.references
            .iter()
            .find(|reference| reference.covers(cell))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HyperlinkActivation {
    None,
    Open,
    Blocked,
}

fn hyperlink_activation(control: bool, click_no_drag: bool, uri: &str) -> HyperlinkActivation {
    if !control || !click_no_drag {
        return HyperlinkActivation::None;
    }
    let allowed = uri.split_once(':').is_some_and(|(scheme, remainder)| {
        (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
            && remainder.starts_with("//")
            && remainder.len() > 2
    }) && !uri
        .chars()
        .any(|character| character.is_control() || character.is_whitespace());
    if allowed {
        HyperlinkActivation::Open
    } else {
        HyperlinkActivation::Blocked
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LocalImageActivation {
    None,
    Preview(PathBuf),
    External(PathBuf),
}

impl LocalImageActivation {
    fn path(&self) -> Option<&std::path::Path> {
        match self {
            Self::None => None,
            Self::Preview(path) | Self::External(path) => Some(path),
        }
    }
}

fn local_image_activation(
    control: bool,
    click_no_drag: bool,
    worker_verified_path: Option<&std::path::Path>,
) -> LocalImageActivation {
    let Some(path) = worker_verified_path.filter(|_| click_no_drag) else {
        return LocalImageActivation::None;
    };
    if control {
        LocalImageActivation::External(path.to_path_buf())
    } else {
        LocalImageActivation::Preview(path.to_path_buf())
    }
}

#[derive(Debug, Default)]
struct ImeCursorThrottle {
    last_sent_at: Option<Instant>,
    last_sent_area: Option<ImeCursorArea>,
    pending: Option<ImeCursorArea>,
}

impl ImeCursorThrottle {
    fn offer(&mut self, area: ImeCursorArea, now: Instant) -> Option<ImeCursorArea> {
        if self.last_sent_area == Some(area) {
            self.pending = None;
            return None;
        }
        if self
            .last_sent_at
            .is_none_or(|last| now.saturating_duration_since(last) >= IME_CURSOR_AREA_INTERVAL)
        {
            self.mark_sent(area, now);
            Some(area)
        } else {
            self.pending = Some(area);
            None
        }
    }

    fn flush_due(&mut self, now: Instant) -> Option<ImeCursorArea> {
        let area = self.pending?;
        if now < self.deadline()? {
            return None;
        }
        self.mark_sent(area, now);
        Some(area)
    }

    fn deadline(&self) -> Option<Instant> {
        self.pending.and(
            self.last_sent_at
                .map(|last| last + IME_CURSOR_AREA_INTERVAL),
        )
    }

    fn mark_sent(&mut self, area: ImeCursorArea, now: Instant) {
        self.last_sent_at = Some(now);
        self.last_sent_area = Some(area);
        self.pending = None;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

// ── T2: what a tab's mark slot says about its session ──
//
// The mock-up hangs four separate channels off one 15px slot (`.ticon-wrap`,
// line 238) and is emphatic that they are separate: breathing says *running*,
// the dot says *finished, and how*, the ring says *how far*, and the dead state
// says *gone*. They coexist because each speaks in its own medium — motion,
// a badge, an arc, a fade — and the whole design falls apart the moment two of
// them are collapsed into one colour.
//
// Everything in this section is a pure function of a `SessionStatus` snapshot
// and a clock. Nothing here draws, and nothing here reads a session: `seats.rs`
// turns these answers into rectangles, and `Runtime` supplies the facts.

/// One claim a session can make on the user's attention, quietest first.
///
/// The order is the whole point, and it is the mock-up's own: `tabDotClass`
/// (lines 1932-1939) tests `await`, then `fail`, then `bell`, then plain
/// unread, and returns at the first hit. Deriving `Ord` over the variants in
/// ascending loudness turns that ladder into `max`, which is what makes a tab's
/// claim the loudest of its members' rather than a hand-rolled cascade that has
/// to be kept in step with the per-session one.
///
/// **The slot above [`Self::Failed`] belongs to `await`** — "an agent is blocked
/// on YOU", the mock-up's loudest claim (line 262). It is deliberately absent
/// here rather than stubbed: no session in this build can report it, and an
/// unreachable variant is a promise the code cannot keep. When the attention
/// queue lands it goes at the end of this enum, wears `--warn` with
/// `fcpulse .9s`, and every consumer below keeps working unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
enum StatusClaim {
    /// Nothing to say — no dot is drawn at all.
    #[default]
    Silent,
    /// Finished, and the tab has not been looked at since (`--accent`).
    Unread,
    /// The bell rang (`--warn`).
    Bell,
    /// Finished with a failing exit code (`--err`).
    Failed,
}

impl StatusClaim {
    /// The dot's fill, or `None` when there is no dot to draw.
    ///
    /// `.unreaddot` is `--accent`, `.fail` is `--err` and `.bell` is `--warn`
    /// (mock-up lines 253-264). All three are opaque and land on the mark, not
    /// on the tab, so none of them needs the palette's compositing treatment.
    fn dot_color(self, palette: &ChromePalette) -> Option<[u8; 3]> {
        match self {
            Self::Silent => None,
            Self::Unread => Some(palette.accent),
            Self::Bell => Some(palette.status_warn),
            Self::Failed => Some(palette.status_err),
        }
    }
}

/// One session's state as the tab strip needs to read it.
#[derive(Clone, Copy, Debug)]
struct SessionFacts {
    status: SessionStatus,
    /// The revision this session had reached the last time the user was
    /// actually looking at it.
    last_seen_revision: u64,
    /// Whether the tab holding this session is the one on screen.
    tab_is_active: bool,
}

impl SessionFacts {
    /// Whether the session has published anything the user has not seen.
    ///
    /// The active tab's own session can never be unread: the user is looking at
    /// it, so "published" and "seen" are the same event. That is not an
    /// optimisation but the definition — without it the tab you are staring at
    /// wears a dot telling you to look at it.
    fn has_unseen_output(self) -> bool {
        !self.tab_is_active && self.status.published_revision > self.last_seen_revision
    }

    /// What this session is claiming right now.
    ///
    /// Transcribed from `stateDotClass` (mock-up lines 1922-1926), including
    /// the two suppressions that make the taxonomy work:
    ///
    /// * **work in flight outranks a finished claim.** A session that is
    ///   running, or that has a progress report on the wire, has not finished,
    ///   so it cannot claim "finished" in either flavour. The mock-up's comment
    ///   at line 1920 is a user ruling in its own right: "an active download is
    ///   still WORK IN FLIGHT: no finished-unread claim until the progress
    ///   ends". The breathing icon and the ring are already saying what is
    ///   happening; a dot would be a third voice on the same fact.
    /// * **a failure is a *kind* of unread.** `fail` is `unread && lastExit ===
    ///   "err"`, not a claim of its own, so looking at the tab retires the red
    ///   dot exactly as it retires the plain one. A failure you have already
    ///   read about is not news.
    ///
    /// The bell is the exception to both: it is latched by the session and
    /// survives whatever else is happening, because a bell is a thing that
    /// *rang*, not a state the session is in.
    fn claim(self) -> StatusClaim {
        let work_in_flight = self.status.working || self.status.progress.is_some();
        let unread = !work_in_flight && self.has_unseen_output();
        if unread && self.status.failure_exit_code.is_some() {
            StatusClaim::Failed
        } else if self.status.bell_latched {
            StatusClaim::Bell
        } else if unread {
            StatusClaim::Unread
        } else {
            StatusClaim::Silent
        }
    }
}

/// How much of a session the user has seen, one frame on.
///
/// Watching a tab *is* seeing it, so the tab on screen carries its ledger
/// forward with every frame it publishes and can never accumulate a backlog.
/// Every other tab's ledger stands still, and the gap that opens between it and
/// the session's own revision is exactly what "unread" measures.
///
/// This is a rule rather than a line inside the event loop because getting it
/// wrong is invisible until the user switches tabs: leave it out and suppress
/// the dot on the active tab instead, and everything looks right until the
/// moment they leave, when the tab they were reading lights up behind them.
fn seen_revision(previous_seen: u64, published: u64, tab_is_active: bool) -> u64 {
    if tab_is_active {
        published
    } else {
        previous_seen
    }
}

/// How opaque a tab's mark is drawn.
///
/// The breath is on the mark and nothing else, and it is *only* a breath while
/// there is something to breathe about. Both of the other answers are a flat
/// `1.0`, and they are flat on purpose:
///
/// * a ring has replaced the mark, and the ring is already reporting "still
///   going" in its own medium — fading it as well would say it twice;
/// * nothing is running, so the mark is simply itself.
///
/// The `!working` case is the one that has to be nailed down rather than left
/// to fall out of a phase calculation. A breath sampled at the wrong moment
/// returns whatever the curve happened to be passing through, so a session that
/// stops working must not be asked where in its breath it was — it must be
/// answered `1.0` outright, by a rule, at every phase and under every motion
/// preference.
fn mark_opacity(working: bool, mark_is_replaced: bool, elapsed: Duration, motion: Motion) -> f32 {
    if mark_is_replaced || !working {
        return 1.0;
    }
    breathe_opacity(elapsed, motion)
}

/// Whether a tab owes the strip a frame, given what it last had drawn.
///
/// This is the question the scheduler has to ask, and asking a *different* one
/// is what put a half-faded icon on screen after `Start-Sleep 8` returned. The
/// old predicate was "is anything moving?", which is the right question for
/// [`Runtime::strip_animation_deadline`] — how long to keep waking up — and the
/// wrong one for whether to draw. The two part company at exactly one moment,
/// and it is the moment that matters: the frame on which motion *stops*.
/// Nothing is moving any more, so the old predicate said "nothing owed" and
/// returned before rebuilding — leaving whatever half-transparent frame the
/// breath happened to end on as the last thing ever drawn.
///
/// Comparing against what was last drawn answers it for every channel at once,
/// because every channel ends up in the same struct: the breath stopping, an
/// indeterminate ring clearing back to its mark, a reduced-motion mark stepping
/// between `.6` and `1.0`, and a dot arriving on a tab that is not moving at
/// all and never was.
///
/// Note there is no `is_animating` clause here, and it would be redundant if
/// there were: an animation that has moved has changed this struct, and one
/// that has not moved has nothing to draw. Continuous wake-ups are the
/// deadline's job, not this one's.
///
/// Written over whatever the channel's drawn state happens to be, because the
/// argument above is about the *shape* of the question and not about tabs: the
/// profile chevron's turn asks it of a quantized angle, and asking it any other
/// way would strand that arrow mid-turn for exactly the reason a mark was
/// stranded mid-breath.
fn tab_owes_frame<T: PartialEq>(last_drawn: Option<T>, showing: T) -> bool {
    last_drawn != Some(showing)
}

/// Whether a tab's latched attention has already been spent by being looked at.
///
/// Watching is consuming (user ruling). A terminal you are sitting in front of
/// does not need a badge telling you to look at it — it has already said
/// everything the badge would repeat, and louder. So the bell and the failure
/// latch retire the moment they arrive on the tab that is both on screen *and*
/// in the focused window, exactly as [`seen_revision`] retires new output for
/// the same tab and for the same reason.
///
/// Both halves of the condition carry weight, and the second is the one worth
/// stating out loud: a window in the background is a window nobody is reading.
/// Clearing on "active tab" alone would silently eat every bell that rang while
/// the user was away in another application — which is the one moment a bell is
/// actually doing its job.
fn attention_is_consumed(tab_is_active: bool, window_is_focused: bool) -> bool {
    tab_is_active && window_is_focused
}

/// The claim a tab wears: the loudest of the claims its sessions make.
///
/// The mock-up's rule, from a user correction it records at line 1930: "a tab
/// must never say less than its panes do". A tab is a lid over sessions the
/// user cannot see, so anything it hides has to surface here or it is lost.
fn loudest_claim(claims: impl IntoIterator<Item = StatusClaim>) -> StatusClaim {
    claims.into_iter().max().unwrap_or_default()
}

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function, evaluated at `x`.
///
/// A CSS timing function is a Bézier whose x axis is *time*, so reading one
/// means inverting x to the curve parameter before evaluating y. There is no
/// closed form for the inversion, so this is Newton's method with a bisection
/// fallback — what a browser does, and what makes this the real curve rather
/// than a smoothstep that merely resembles it.
///
/// The two curves the tab strip needs are the two CSS keywords the mock-up
/// names: [`EASE_IN_OUT`] for the breath and [`EASE`] for the arc's transition.
/// They are genuinely different shapes — `ease` is asymmetric, leaving quickly
/// and arriving slowly — so they are two sets of control points through one
/// solver rather than one curve standing in for both.
///
/// CSS constrains both control points' `x` to `0..=1`, which makes x monotonic
/// in t and the inversion single-rooted.
pub(crate) fn cubic_bezier(x: f32, control: [f32; 4]) -> f32 {
    let [x1, y1, x2, y2] = control;
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // B(t) for a unit cubic Bézier whose first and last points are 0 and 1.
    let axis = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let slope = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };
    let mut t = x;
    let mut solved = None;
    for _ in 0..8 {
        let error = axis(t, x1, x2) - x;
        if error.abs() < 1e-6 {
            solved = Some(t);
            break;
        }
        let derivative = slope(t, x1, x2);
        // A flat tangent would throw Newton off the interval entirely; hand
        // those cases to the bisection rather than clamping to a wrong root.
        if derivative.abs() < 1e-6 || !(0.0..=1.0).contains(&t) {
            break;
        }
        t -= error / derivative;
    }
    let t = solved.unwrap_or_else(|| {
        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        let mut t = x;
        for _ in 0..40 {
            let value = axis(t, x1, x2);
            if (value - x).abs() < 1e-6 {
                break;
            }
            if value < x {
                low = t;
            } else {
                high = t;
            }
            t = (low + high) / 2.0;
        }
        t
    });
    axis(t, y1, y2)
}

/// CSS `ease-in-out`, worn by `@keyframes breathe`.
const EASE_IN_OUT: [f32; 4] = [0.42, 0.0, 0.58, 1.0];
/// CSS `ease`, worn by the progress arc's `transition`.
pub(crate) const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];

/// `.ticon.working { animation: breathe 1.7s ease-in-out infinite }`.
///
/// `@keyframes breathe { 0%, 100% { opacity: 1 } 50% { opacity: .28 } }` — one
/// keyframe pair, so the cycle is two eased halves: down over the first half,
/// back up over the second. Easing each half separately is what CSS does and is
/// why the curve is smooth at the trough and *cornered* at the top: at 0% the
/// animation restarts, and `ease-in-out` starts flat, so the breath rests at
/// full opacity for an instant every cycle.
///
/// With animations turned off the breath collapses to one held value rather
/// than to nothing (mock-up line 1927): the session is still working, and that
/// still has to be visible.
fn breathe_opacity(elapsed: Duration, motion: Motion) -> f32 {
    if motion == Motion::Reduced {
        return WINDOW_TAB_BREATHE_REDUCED_OPACITY;
    }
    let period = WINDOW_TAB_BREATHE_PERIOD_MS as f32;
    let phase = (elapsed.as_secs_f32() * 1000.0).rem_euclid(period) / period;
    let (from, to, half) = if phase < 0.5 {
        (1.0, WINDOW_TAB_BREATHE_MIN_OPACITY, phase * 2.0)
    } else {
        (WINDOW_TAB_BREATHE_MIN_OPACITY, 1.0, (phase - 0.5) * 2.0)
    };
    from + (to - from) * cubic_bezier(half, EASE_IN_OUT)
}

/// Whether the system wants animation at all.
///
/// Windows states this as `SPI_GETCLIENTAREAANIMATION`, which is the same
/// preference a browser reports as `prefers-reduced-motion` — the setting
/// behind Settings → Accessibility → Visual effects → Animation effects.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Motion {
    #[default]
    Full,
    Reduced,
}

impl Motion {
    /// Map `SPI_GETCLIENTAREAANIMATION` — and a failure to read it — to a
    /// preference.
    ///
    /// `Some(true)` is "animation is wanted"; `Some(false)` is the
    /// accessibility setting turned on, which is what CSS calls `reduce`. The
    /// inversion between the two spellings is the whole reason this mapping is
    /// named and pinned rather than written inline at the call site.
    fn from_client_area_animation(enabled: Option<bool>) -> Self {
        match enabled {
            Some(false) => Self::Reduced,
            // A read that failed says nothing about what the user wants, and
            // the preference is opt-in, so the default stands.
            Some(true) | None => Self::Full,
        }
    }
}

/// Ask the system whether it wants animation, once.
///
/// The polarity is the trap and the reason this is its own function with its
/// own pin: `SPI_GETCLIENTAREAANIMATION` is `TRUE` when animation is *wanted*,
/// while `prefers-reduced-motion: reduce` matches when it is *not*. Reading the
/// Win32 answer straight into a "reduced" flag inverts the accessibility
/// setting — the one bug in this area that harms exactly the users it was meant
/// to serve, and that no visual review would catch on a machine with the
/// default setting.
///
/// A system that cannot answer gets the default, which is animation: the
/// preference is opt-in, and a failed read is not a request for less motion.
fn read_motion_preference() -> Motion {
    Motion::from_client_area_animation(bt_platform::client_area_animation_enabled().ok())
}

/// `.pring.indeterminate { animation: pring-spin 1.1s linear infinite }`.
///
/// A fixed arc (`stroke-dasharray: 13 40.4`, line 283) carried around the ring
/// at a constant rate, which is what "indeterminate" means: the shape says how
/// much is done — nothing knowable — and the motion says it is still going.
///
/// Stopped, it holds at 12 o'clock rather than vanishing (line 287 turns the
/// animation off and leaves the arc): a ring with no arc at all is a ring
/// reporting 0%, which is a different and false claim.
fn indeterminate_start_milliturns(elapsed: Duration, motion: Motion) -> u16 {
    if motion == Motion::Reduced {
        return 0;
    }
    let period = WINDOW_TAB_RING_SPIN_PERIOD_MS as f32;
    let phase = (elapsed.as_secs_f32() * 1000.0).rem_euclid(period) / period;
    (phase * 1000.0).round().rem_euclid(1000.0) as u16
}

/// How far round the ring a progress report reaches, in thousandths of a turn.
fn sweep_milliturns(percent: u8) -> u16 {
    u16::from(percent.min(100)) * 10
}

/// The arc a [`ProgressState`] asks for: its colour, and how much of the ring
/// it covers.
///
/// `last_sweep` carries the ring's current reading, and it is what answers the
/// two states that can arrive *without* a percentage. `OSC 9;4` states 2 and 4
/// mark a run as failed or paused; the percentage is optional because the state
/// is a change to a run already in progress, and the number that was already on
/// the wire still stands. Keeping it is therefore the protocol's own reading —
/// and a great deal better than the alternatives, which are to invent a number
/// or to collapse the arc to nothing and report a failure as 0%.
///
/// Only when a run has *never* reported a percentage does the ring fall back to
/// a full turn, because at that point there is no reading to keep and a bare
/// track would say "tracking something" while showing nothing at all.
fn ring_arc(
    state: ProgressState,
    last_sweep: Option<u16>,
    elapsed: Duration,
    motion: Motion,
    palette: &ChromePalette,
) -> RingArc {
    let held = || last_sweep.unwrap_or(1000);
    match state {
        ProgressState::Normal(percent) => RingArc {
            color: palette.accent,
            start_milliturns: 0,
            sweep_milliturns: sweep_milliturns(percent),
            animating: false,
        },
        ProgressState::Error(percent) => RingArc {
            color: palette.status_err,
            start_milliturns: 0,
            sweep_milliturns: percent.map_or_else(held, sweep_milliturns),
            animating: false,
        },
        ProgressState::Paused(percent) => RingArc {
            color: palette.status_pause,
            start_milliturns: 0,
            sweep_milliturns: percent.map_or_else(held, sweep_milliturns),
            animating: false,
        },
        ProgressState::Indeterminate => RingArc {
            color: palette.accent,
            start_milliturns: indeterminate_start_milliturns(elapsed, motion),
            sweep_milliturns: (WINDOW_TAB_RING_INDETERMINATE_TURNS * 1000.0).round() as u16,
            animating: motion == Motion::Full,
        },
    }
}

/// One ring's live arc.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RingArc {
    color: [u8; 3],
    start_milliturns: u16,
    sweep_milliturns: u16,
    /// Whether this arc moves on its own and therefore owes the next frame.
    animating: bool,
}

/// A ring's sweep easing toward a new reading.
///
/// `.pring .arc { transition: stroke-dashoffset .3s ease }` (line 279). A
/// progress report arrives in steps — 0, then 12, then 30 — and without this
/// the ring snaps between them; the design's answer is that the *number* jumps
/// and the arc does not.
///
/// Deliberately not disabled under reduced motion: the mock-up's own
/// `prefers-reduced-motion` block (lines 286-289) turns off the spin and the
/// pulse and leaves this transition alone. A 300ms ease is the arc arriving at
/// a value, not something travelling across the screen.
#[derive(Clone, Copy, Debug)]
struct SweepTween {
    from: u16,
    to: u16,
    started: Instant,
}

impl SweepTween {
    /// Where the arc is now, and whether it is still moving.
    fn sample(self, now: Instant) -> (u16, bool) {
        let duration = Duration::from_millis(WINDOW_TAB_RING_SWEEP_TRANSITION_MS);
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed >= duration {
            return (self.to, false);
        }
        let progress = elapsed.as_secs_f32() / duration.as_secs_f32();
        let eased = cubic_bezier(progress, EASE);
        let from = f32::from(self.from);
        let to = f32::from(self.to);
        ((from + (to - from) * eased).round() as u16, true)
    }
}

/// A two-state control easing between "not there" and "there", over a span its
/// own declaration names.
///
/// Written for the pin's zero-width expansion - `width 0 -> 17px` and
/// `margin-left -8 -> 0` as one continuous layout change (mock-up 334-349),
/// which is a *reveal* rather than a fade: the hidden control takes no room, so
/// the badge beside it docks against the close affordance with no dead gap, and
/// hovering widens the control in while the badge slides aside. A fade would
/// have to reserve the room permanently and every unhovered tab would carry a
/// hole.
///
/// The dock preview's `opacity .1s` is the second user, and it is why the span
/// is a *field*. Both are one declaration easing between two states on the same
/// curve, and the only thing that differs is how long the mock-up gives it - so
/// the duration travels with the tween rather than being reached for at the
/// sample, where two readers of one tween could pick different numbers. There is
/// deliberately no `Default`: a reveal with no span is either an instant nobody
/// asked for, or somebody else's 160ms borrowed by accident.
#[derive(Clone, Copy, Debug)]
struct RevealTween {
    from: f32,
    to: f32,
    started: Option<Instant>,
    span: Duration,
}

impl RevealTween {
    /// A reveal that is fully out, easing over `span` when it is asked to move.
    fn over(span: Duration) -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            started: None,
            span,
        }
    }

    /// Aim at `target`, keeping whatever the current position is as the new
    /// start so a reversal mid-flight turns around from where it actually is
    /// rather than snapping to an end it never reached.
    fn retarget(&mut self, target: f32, now: Instant, motion: Motion) {
        if self.to == target {
            return;
        }
        let (current, _) = self.sample(now, motion);
        *self = Self {
            from: current,
            to: target,
            // `prefers-reduced-motion` kills this transition outright (mock-up
            // 359-361), so under Reduced the control is simply *there* or not.
            // Unlike the progress arc, whose motion carries a reading, this one
            // carries nothing but polish.
            started: (motion == Motion::Full).then_some(now),
            span: self.span,
        };
    }

    /// Where the reveal is now, and whether it is still moving.
    fn sample(self, now: Instant, motion: Motion) -> (f32, bool) {
        let Some(started) = self.started.filter(|_| motion == Motion::Full) else {
            return (self.to, false);
        };
        let elapsed = now.saturating_duration_since(started);
        let duration = self.span;
        if elapsed >= duration {
            return (self.to, false);
        }
        let progress = elapsed.as_secs_f32() / duration.as_secs_f32();
        let eased = cubic_bezier(progress, EASE);
        (self.from + (self.to - self.from) * eased, true)
    }
}

/// `.chevbtn svg { transition: transform 140ms cubic-bezier(.2,0,0,1) }` —
/// the profile picker's arrow turning over (mock-up 415-420).
const CHEVRON_TURN: Duration = Duration::from_millis(140);

/// The `˅` beside the `+`, turning over to say where its list went.
///
/// It carries a fraction and not an angle: 0.0 is the resting arrow, 1.0 is
/// `.chevbtn.open svg { transform: rotate(180deg) }`, and the degrees are the
/// mark's business rather than this one's — which is also what keeps the
/// quantization in one place, at [`marks::ChromeMark::chevron`].
///
/// The same shape as [`RevealTween`], and for the same reason: both are
/// two-state controls that can be told to go back before they arrive, and both
/// answer by turning around from where they *are*. The mock-up asks for a
/// transition on a property, and a CSS transition interrupted mid-flight is
/// restarted from the current computed value — the arrow never jumps to an end
/// it did not reach, which is the whole difference between a turn and a swap.
///
/// (CSS also shortens such a reversal by its "reversing shortening factor",
/// so a turn undone at 10% takes 10% of the time back. Not reproduced here,
/// deliberately and consistently with `RevealTween` beside it: the curve's own
/// long tail already makes the last degrees nearly free, and a fixed 140ms is
/// the number the mock-up actually writes down.)
#[derive(Clone, Copy, Debug, Default)]
struct ChevronTurn {
    from: f32,
    to: f32,
    started: Option<Instant>,
}

impl ChevronTurn {
    /// Turn towards `open`, from wherever the arrow currently points.
    fn retarget(&mut self, open: bool, now: Instant, motion: Motion) {
        let target = f32::from(u8::from(open));
        if self.to == target {
            return;
        }
        let (current, _) = self.sample(now, motion);
        *self = Self {
            from: current,
            to: target,
            // `@media (prefers-reduced-motion: reduce) { .chevbtn svg {
            // transition: none } }` (mock-up 420). `none` on a transition is
            // not a shorter transition — it is the terminal value at once, and
            // with no `started` that is exactly what `sample` reports.
            started: (motion == Motion::Full).then_some(now),
        };
    }

    /// How far through the turn the arrow is, and whether it is still turning.
    fn sample(self, now: Instant, motion: Motion) -> (f32, bool) {
        let Some(started) = self.started.filter(|_| motion == Motion::Full) else {
            return (self.to, false);
        };
        let elapsed = now.saturating_duration_since(started);
        if elapsed >= CHEVRON_TURN {
            return (self.to, false);
        }
        let progress = elapsed.as_secs_f32() / CHEVRON_TURN.as_secs_f32();
        let eased = cubic_bezier(progress, GRAB_EASE);
        (self.from + (self.to - self.from) * eased, true)
    }
}

/// `transform .16s cubic-bezier(.2, 0, 0, 1)` — the one easing the whole reorder
/// is drawn with (`GRAB_EASE`, mock-up 6570).
const TAB_FLIP: Duration = Duration::from_millis(160);
/// The curve of that transition. Not one of the CSS keywords the strip already
/// had, so it is a third set of control points through the same solver.
const GRAB_EASE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
/// `animation: tab-land .2s cubic-bezier(.2, 0, 0, 1)` (mock-up 967).
const TAB_LAND: Duration = Duration::from_millis(200);

/// A tab sliding back to the slot its index gives it.
///
/// This is FLIP, in the one form this strip needs it: the order changes first,
/// then the tab is put back where it *was* with a translation, and the
/// translation is released. Nothing about the layout is animated — only the
/// difference between where a tab is drawn and where it belongs, which decays to
/// zero.
///
/// It is one mechanism for two motions the mock-up writes separately, because
/// they are the same motion: `playStripFlip` starts a displaced neighbour at the
/// slot it just left (6584-6598), and `releaseGrabbed` starts the tab you let go
/// of at wherever your hand left it (6622-6635). Both then run the same curve for
/// the same 160ms down to the same zero.
#[derive(Clone, Copy, Debug, Default)]
struct FlipTween {
    /// How far from its slot the tab starts this slide, in physical pixels.
    from: f32,
    started: Option<Instant>,
}

impl FlipTween {
    /// The tab's slot has moved by `delta` physical pixels (old left minus new
    /// left); start it over from where it is *now*.
    ///
    /// Reading the current offset first is what the mock-up's `snapshotStrip`
    /// does by measuring `getBoundingClientRect`, which includes the live
    /// transform: a tab displaced a second time while the first slide is still
    /// running starts the new one from where it actually is, not from a slot it
    /// never reached.
    fn displace(&mut self, delta: f32, now: Instant, motion: Motion) {
        let from = self.sample(now, motion).0 + delta;
        *self = Self {
            from,
            started: (motion == Motion::Full && from != 0.0).then_some(now),
        };
    }

    /// Where the tab is drawn relative to its slot, and whether it is still
    /// moving.
    fn sample(self, now: Instant, motion: Motion) -> (f32, bool) {
        let Some(started) = self.started.filter(|_| motion == Motion::Full) else {
            return (0.0, false);
        };
        let elapsed = now.saturating_duration_since(started);
        if elapsed >= TAB_FLIP {
            return (0.0, false);
        }
        let progress = elapsed.as_secs_f32() / TAB_FLIP.as_secs_f32();
        (self.from * (1.0 - cubic_bezier(progress, GRAB_EASE)), true)
    }
}

/// `@keyframes tab-land` running down (mock-up 955-968).
///
/// The keyframe writes only a `from`, so this carries only how much of that
/// `from` is left: the animation ends at whatever the tab already is, and needs
/// to know nothing about the tab's real styling to get there.
///
/// `prefers-reduced-motion` turns it off outright (line 968) — unlike the FLIP
/// beside it, which the mock-up's reduced-motion block deliberately does not
/// mention. Off means the terminal state immediately, which for an animation
/// that only has a `from` is simply the tab, unwashed.
#[derive(Clone, Copy, Debug, Default)]
struct LandTween {
    started: Option<Instant>,
}

impl LandTween {
    fn start(&mut self, now: Instant, motion: Motion) {
        self.started = (motion == Motion::Full).then_some(now);
    }

    /// How much of the wash is left, and whether it is still running.
    fn sample(self, now: Instant, motion: Motion) -> (f32, bool) {
        let Some(started) = self.started.filter(|_| motion == Motion::Full) else {
            return (0.0, false);
        };
        let elapsed = now.saturating_duration_since(started);
        if elapsed >= TAB_LAND {
            return (0.0, false);
        }
        let progress = elapsed.as_secs_f32() / TAB_LAND.as_secs_f32();
        (1.0 - cubic_bezier(progress, GRAB_EASE), true)
    }
}

/// K115 — how far from its slot a grabbed tab is drawn, given where the hand
/// wants it.
///
/// The strip has one axis, so the tab travels in `x` and the row it lives in does
/// not move at all. `viewport` is the strip's own clip box: a tab held past
/// either end stops at the end rather than being carried out over the caption
/// buttons or off the window's left edge, and the *clamped* position is what the
/// reorder is then judged from — the tab you can see is the tab that decides.
///
/// The upper bound is written as a `max` of the lower one rather than trusted,
/// because a strip narrower than one tab has none: the two bounds cross, and the
/// leading edge is the one that must win.
fn grabbed_offset(slot_left: f32, tab_width: f32, viewport: [f32; 2], want_left: f32) -> f32 {
    let [view_left, view_right] = viewport;
    want_left.clamp(view_left, (view_right - tab_width).max(view_left)) - slot_left
}

/// The nearest slot to `to` that a tab now at `from` may legally occupy — F57
/// stated for a move that geometry did not choose.
///
/// The reorder gets the partition for free, because it only ever steps one slot
/// and asks before each step. A restore does not: Esc names a slot outright, and
/// between the drag starting and Esc arriving a *pinned* tab may have been
/// reaped, which shifts every index after it by one. So the same rule is applied
/// the same way — walk toward the target and stop before the seam.
fn partition_clamped(pinned: &[bool], from: usize, to: usize) -> usize {
    let mut at = from;
    while at != to {
        let next = if at < to { at + 1 } else { at - 1 };
        if pinned.get(next) != pinned.get(from) {
            break;
        }
        at = next;
    }
    at
}

/// The shape the pointer wears — K113 included.
///
/// "You grabbed this with the pointing finger, and the cursor changing shape
/// mid-drag would say something happened when nothing did" (mock-up 1710-1711).
/// The mock-up pins the shape to `pointer` because in a browser an un-pinned
/// cursor flickers through three shapes on the way to the drop; here the shape a
/// tab press starts with is the ordinary arrow, so pinning it means keeping
/// *that* — the point is that it does not change, not which one it is.
fn pointer_cursor(
    dragging_tab: bool,
    divider_axis: Option<bt_layout::Axis>,
) -> winit::window::CursorIcon {
    use winit::window::CursorIcon;
    if dragging_tab {
        return CursorIcon::Default;
    }
    match divider_axis {
        Some(bt_layout::Axis::Row) => CursorIcon::EwResize,
        Some(bt_layout::Axis::Col) => CursorIcon::NsResize,
        None => CursorIcon::Default,
    }
}

/// What is in the hand (J111).
///
/// The mock-up's line 6352 is the whole argument for this enum existing at all:
/// `drag & dock (tabs and panes share one engine)`. One state machine, one
/// threshold, one ghost, one teardown — and the *only* thing that varies between
/// a tab and a pane is what a landing does with them, which is not this slice's
/// question.
///
/// Both variants key on identity rather than on position, for the same reason:
/// the strip renumbers under a finger that has not moved, and a seat tree can be
/// re-solved between two pointer moves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DragSource {
    Tab(TabId),
    /// A pane, taken by its head (J118, mock-up 5835-5840).
    Pane(SeatId),
}

/// Where a drag would land if the hand opened right now — the engine's entire
/// knowledge of drop targets.
///
/// **This is the seam U5 plugged into.** The engine asks
/// [`Runtime::survey_drop`] on every pointer move and stores the answer; it
/// never asks *why*. The strip-rectangle test (K123), the 48px rim (K130) and
/// the pane zones (K133/K134) are variants here and arms of that one function,
/// and nothing in the state machine below had to learn about any of them.
///
/// `None` — no landing — is not an error state and not a fifth variant. It is
/// what a pointer over open air answers, what a pane held over itself answers
/// (K135), and what J120 is about.
///
/// **What a landing carries, and what it deliberately does not.** Identities and
/// a direction, never a rectangle. U6 draws the preview from the same solved
/// layout this was answered against and U7 commits from the same tree; a
/// rectangle copied in here would be a third opinion, and the first one to go
/// stale (A12, T228).
///
/// **`Refused` is not among these, and that is a finding rather than an
/// omission.** The mock-up has a refused state and it is loud about needing one
/// (M147: a silent refusal makes "this pane is too narrow to split" and "this
/// app is broken" look identical). But every producer of it is somewhere else.
/// `refused` is set by `!fits` — `planFits` over the tree the drop *would*
/// build (M155, H93/H94) — which is the plan computation U6 owns; by the
/// folder/file centre verbs (L141/L142), which need a drag source this build
/// cannot yet have; and by `refuseFocusStage`, whose state no code in this crate
/// constructs. The two refusals K itself names are not refusals at all: a pane
/// held over its own rectangle (K135, 7101) and a tab held over its own layout
/// (K129, 6934) both call `hidePreview()` and leave `drag.target` null. Adding a
/// variant here now would be a variant nothing can answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DropLanding {
    /// The strip has it, at this slot: `reorderWhileDragging` followed by
    /// `drag.target = { reordered: true }` (mock-up 6835-6836).
    ///
    /// The reorder is applied *live*, so by the time a release arrives there is
    /// nothing left to commit — which is why the mock-up's own commit table
    /// treats `reordered` beside "no target at all" (7202). They are not the
    /// same thing here: one has already happened, the other never will.
    StripReorder { slot: usize },
    /// **K124/N157** — a *pane* over the strip: it leaves the tree and becomes
    /// its own tab at this slot (`extractPaneToTab`).
    ///
    /// Only offered while the tab has more than one pane. A lone pane torn into
    /// its own tab would be the same pane in the same strip position it already
    /// occupies, with an empty tab left behind (G84 forbids the tree being
    /// emptied), so the mock-up asks `paneCount > 1` before it will draw
    /// anything at all (6789).
    StripExtract { slot: usize },
    /// **K130/G82** — the layout's own rim: the *root* splits on this side.
    ///
    /// No target seat, and that absence is the entire point of the gesture.
    /// With two panes stacked, every zone on screen belongs to the top one or
    /// the bottom one, so a third pane can only ever join one of them — "put
    /// this beside all of it" needs the root split, and the root had no edge to
    /// aim at until the rim gave it one (G83).
    RootRim { edge: seats::DropEdge },
    /// **K134/L136** — the outer 35% of one pane: that pane splits on this side.
    SeatEdge {
        target: SeatId,
        edge: seats::DropEdge,
    },
    /// **K134/L136** — a pane's middle: its place is taken.
    ///
    /// One shape, two sentences, and which one it is depends on what is in the
    /// hand rather than on where the pointer is: a pane swaps payloads with the
    /// target (L138) and a tab replaces it (L139). That is why L137 rules the
    /// centre box must carry *words* — the geometry is identical and the
    /// outcomes are not.
    SeatCentre { target: SeatId },
}

impl DropLanding {
    /// Whether this landing is already showing the user what it means, so the
    /// ghost must get out of the way.
    ///
    /// `drag.ghost.style.opacity = "0"` with the mock-up's own reason beside it:
    /// "the tab itself is the feedback" (6837). Two labels saying the same name,
    /// one under the pointer and one in the slot it is about to take, is the
    /// drag telling you twice — and the one in the slot is the one that is
    /// telling you *where*.
    ///
    /// True for both strip landings and neither of the layout ones, which is the
    /// mock-up's split exactly: the strip yields the ghost because the strip
    /// draws a stand-in *in the run* — "the preview in the strip is the ghost
    /// now" (6792), written beside the pane case rather than the tab one. A rim
    /// or a pane zone draws its box over the layout instead, far from the
    /// pointer and saying something the ghost does not, so the two coexist.
    fn shows_itself(self) -> bool {
        match self {
            Self::StripReorder { .. } | Self::StripExtract { .. } => true,
            Self::RootRim { .. } | Self::SeatEdge { .. } | Self::SeatCentre { .. } => false,
        }
    }

    /// The aim this landing was read off, for the three that are aimed at the
    /// layout — and `None` for the two the strip answered.
    ///
    /// The exact inverse of [`landing_for_aim`], and written as one function so
    /// that it stays one: a plan is built from an aim, and re-deriving "which
    /// pane, which side" from a landing at each of U6's call sites is how the
    /// preview and the drop would come to disagree about the very question they
    /// are both answering. A strip landing has no aim to give back because it
    /// never had one — the strip is a surface, not a rectangle inside the layout.
    fn layout_aim(self) -> Option<seats::LayoutAim> {
        match self {
            Self::StripReorder { .. } | Self::StripExtract { .. } => None,
            Self::RootRim { edge } => Some(seats::LayoutAim::Rim(edge)),
            Self::SeatEdge { target, edge } => Some(seats::LayoutAim::SeatEdge(target, edge)),
            Self::SeatCentre { target } => Some(seats::LayoutAim::SeatCentre(target)),
        }
    }

    /// The pane a refusal is traced onto (M147), or `None` for the rim.
    ///
    /// The rim aims at the layout as a whole, so what it will not cut is the
    /// whole layout — there is no pane to point at, and pointing at one would be
    /// naming a pane the gesture was never about.
    fn aimed_at(self) -> Option<SeatId> {
        match self {
            Self::SeatEdge { target, .. } | Self::SeatCentre { target } => Some(target),
            Self::StripReorder { .. } | Self::StripExtract { .. } | Self::RootRim { .. } => None,
        }
    }

    /// **L137 — the centre says its name.**
    ///
    /// An edge says "split" by its shape: the box takes half the pane and the
    /// half it takes is the half you aimed at. The centre's box is the *same*
    /// blue rectangle and means something else entirely, and which of the two
    /// things it means depends on what is in your hand rather than on where the
    /// pointer is — a pane trades payloads with the target (L138) and a tab takes
    /// its place outright (L139). Two outcomes that far apart cannot be left to
    /// geometry that is identical in both.
    ///
    /// Every other zone answers with nothing, and that is a rule rather than an
    /// omission: a word on a box whose shape already said it is a second voice
    /// saying the same thing, and the first one to be believed when they drift.
    fn caption(self, source: DragSource) -> &'static str {
        match (self, source) {
            (Self::SeatCentre { .. }, DragSource::Pane(_)) => "Swap panes",
            (Self::SeatCentre { .. }, DragSource::Tab(_)) => "Replace pane",
            _ => "",
        }
    }
}

/// The part of a drag that only one kind of source has: a tab is carried by the
/// strip itself, a pane is carried by nothing but the ghost.
///
/// Written as an enum rather than as four `Option`s on one struct, because the
/// asymmetry is real and permanent — a pane has no slot to be offset from and
/// never will. Flattening it would put four fields on every pane drag that are
/// meaningless for the whole of its life.
#[derive(Clone, Copy, Debug)]
enum DragCarry {
    /// K114-K118: the tab is drawn out of its slot and the strip reorders under
    /// it.
    Tab(TabCarry),
    /// J118: the tree is not touched while a pane is in the air. The pane stays
    /// exactly where it is and the ghost is the only thing that moves — which is
    /// also why a pane drag that comes to nothing has nothing to undo.
    Pane,
}

/// A tab being dragged along the strip (K111, K114-K118).
#[derive(Clone, Copy, Debug)]
struct TabCarry {
    /// Where inside the tab's own body the pointer took hold, in physical
    /// pixels. It is what makes the tab hang off the pointer where you picked it
    /// up instead of jumping its own left edge under the cursor.
    grab_dx: f64,
    /// The slot the tab held when the drag began. J120 puts it back here.
    origin: usize,
    /// **N163/J107** — the tab that was *showing* when the drag began, which is
    /// not always the tab in hand: a press on a background tab activates it so
    /// you can see what you have picked up.
    ///
    /// The mock-up carries the same pair (`drag.wsId` beside `drag.homeWs`,
    /// 6911-6918) and needs to, because the two halves of the gesture want
    /// different tabs. Leaving the strip says "now place A somewhere in the
    /// layout I was in", and the layout you were in is this one.
    home: TabId,
    /// How far from its slot the tab is currently drawn, in physical pixels —
    /// [`Runtime::track_grabbed`]'s answer, kept so that the frame that paints it
    /// and the frame that lets go of it agree.
    offset: f32,
    /// Whether this drag has actually changed the strip's order.
    ///
    /// A drag that travelled 8px and came back has decided nothing, and the
    /// session file records order: writing it anyway would turn a gesture the
    /// user abandoned into a "meaningful change" (§5.1).
    moved: bool,
}

/// A gesture in flight — the `dragging` state of J's state machine.
///
/// The other three states are not variants here, and that is the honest shape of
/// them. *Idle* is `None`. *Pressed* is a latch that has not fired yet, and it
/// lives on the press ([`TabPress::latch`], [`PanePress::latch`]) because what a
/// press is holding differs by source while a drag does not. *Cancelled* is not a
/// state at all but an exit — `Runtime::cancel_drag` unwinds to idle in one
/// call, and a "cancelled" resting state would be a drag that has stopped being a
/// drag while still occupying the field that means one is happening.
#[derive(Clone, Copy, Debug)]
struct Drag {
    source: DragSource,
    carry: DragCarry,
    /// Where the pointer is, in physical pixels. The ghost hangs off this, and
    /// it is stored rather than re-read because the frame that paints the ghost
    /// is not the event that moved it.
    pointer: PhysicalPosition<f64>,
    /// What [`Runtime::survey_drop`] answered on the last pointer move.
    landing: Option<DropLanding>,
}

impl Drag {
    /// The tab this drag is carrying, if it is carrying one.
    ///
    /// Paired with [`Self::tab_carry`] rather than folded into it: the identity
    /// survives things the carry does not care about, and several callers want
    /// only one of the two.
    fn tab(&self) -> Option<TabId> {
        match self.source {
            DragSource::Tab(tab) => Some(tab),
            DragSource::Pane(_) => None,
        }
    }

    fn tab_carry(&self) -> Option<TabCarry> {
        match self.carry {
            DragCarry::Tab(carry) => Some(carry),
            DragCarry::Pane => None,
        }
    }

    /// Whether the ghost is drawn for this drag right now (J114).
    fn ghost_is_shown(&self) -> bool {
        !self.landing.is_some_and(DropLanding::shows_itself)
    }
}

/// The dock drawing on screen: a plan, the question it answers, and how far the
/// box has faded in (M144-M155).
struct DropPreview {
    /// Everything [`seats::Seats::plan_drop`] is a function of.
    ///
    /// **This is the cache, and it is a cache over a *computation*.** The plan is
    /// recomputed when any of these changes and reused when none of them does,
    /// which is what "only re-plan when the landing moves" means without the
    /// stale answers that phrasing invites — a window resized under a still
    /// pointer, or a DPI change mid-drag, changes the plan without changing the
    /// landing, and keying on the landing alone would leave the promise on screen
    /// describing a layout that no longer exists. T223's ban on a second
    /// estimating solver is not weakened by remembering the first one's answer;
    /// it would be weakened by remembering it past the question.
    inputs: PlanInputs,
    plan: seats::DropPlan,
    /// `#dock-preview { transition: opacity .1s ease }` with `.show` toggling it
    /// (mock-up 1663-1665).
    ///
    /// The *only* thing about this drawing that is allowed to take time, and
    /// M148 is why. The mock-up also declares `left/top/width/height .12s`, and
    /// those transitions are unreachable by construction: the box's geometry is a
    /// function of the promise — `zone:fits` — so any move of the box is a change
    /// of the promise, and `promise()` puts `.snap` on before the new geometry
    /// lands. A glide could only ever run while the answer stayed the same and
    /// the box moved anyway, which within one drag cannot happen. So the box
    /// snaps, always, and there is no rectangle tween here to be kept honest —
    /// the implementation that cannot lag is the one M148 asks for, since a
    /// preview that lags is not a soft transition but a stale promise about what
    /// happens if you let go *right now*.
    reveal: RevealTween,
}

/// `#dock-preview { transition: opacity .1s ease }` (mock-up 1663).
///
/// The one duration in this drawing. It is deliberately shorter than every other
/// transition the chrome runs — the pin's 160, the chevron's 140 — because it is
/// the only one attached to an answer rather than to a control: what a box that
/// says "let go here" owes is to be there, and a hundred milliseconds is about
/// the least a fade can take and still be a fade rather than a flicker.
const DOCK_PREVIEW_FADE: Duration = Duration::from_millis(100);

/// The inputs a [`seats::DropPlan`] is a function of — see [`DropPreview::inputs`].
#[derive(Clone, Debug, PartialEq)]
struct PlanInputs {
    landing: DropLanding,
    source: DragSource,
    /// The tree the drop edits. Carried in full rather than as a revision
    /// counter, because there is no revision counter to be wrong: two trees that
    /// compare equal solve to the same rectangles (D2), which is exactly the
    /// question being asked.
    tree: LayoutNode,
    /// The arriving tab's own layout, when a tab is what is arriving (M156①).
    cargo: Option<LayoutNode>,
    viewport: LogicalRect,
    scale_ppm: u32,
}

/// What letting go does — the mock-up's commit table (7202-7231) reduced to the
/// one question this slice can answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DragRelease {
    /// A landing answered, so the landing decides.
    Commit,
    /// **U7** — a landing in the *layout* answered: adopt the tree the plan
    /// built (L136-L140, G81-G83).
    ///
    /// Separate from [`Self::Commit`] because the two words mean opposite things
    /// about work already done. A strip reorder was applied live, slot by slot,
    /// as the tab travelled: committing it is letting it stand. A layout drop has
    /// changed nothing at all — the tree was untouched for the whole gesture —
    /// so this is the release that does the work rather than the one that keeps
    /// it.
    Land,
    /// **J120.** Nothing answered, so the carried thing slides back to the slot
    /// the drag began in, displaced neighbours travelling home with it.
    Home,
}

/// J120, stated once.
///
/// **Ruling, recorded 2026-08-08.** The mock-up leaves a drag that landed
/// nowhere exactly where the live reorder last put it (`!d.target` falls into
/// the same arm as `d.target.reordered` and returns, 7202-7208). The native
/// build slides it home instead, and the argument is the mock-up's own: a
/// reorder performed on the way to a drop is part of that drop's gesture, so a
/// release that commits nothing must not keep half of what it did on the way.
///
/// It is deliberately **not** the same verdict as a cancel even though the two
/// share the motion. A cancel is the user retracting the gesture; this is the
/// gesture finishing and finding nowhere to be. Nothing downstream distinguishes
/// them *today* — both go home and neither writes the session — and that is a
/// fact about this slice rather than a claim about the ruling: the moment a
/// landing exists that a release can commit and an Esc must not, the two callers
/// are already separate.
/// What an aim means once you know what is in the hand — K135, stated once.
///
/// **Never onto yourself.** A pane held over its own rectangle has no landing in
/// any zone: `if (drag.kind === "pane" && drag.leafId === leafId) { hidePreview();
/// return; }` (7101). Splitting a pane against itself and swapping it with
/// itself are both the identity, and the honest report of a gesture that would
/// do nothing is that there is nothing there.
///
/// The rim is exempt and could not be otherwise. It docks against the *whole*
/// layout (G82/G83), so it has no target seat that could be yours — the pane in
/// your hand is part of the whole rather than the thing being aimed at, and
/// dragging your own pane out to the rim is exactly how you move it beside
/// everything else.
///
/// A free function rather than a method because it needs nothing from the
/// window: it is the sentence "who may land on what", and separating it from
/// "what is under the pointer" is what lets each be tested against its own
/// inputs.
fn landing_for_aim(source: DragSource, aim: seats::LayoutAim) -> Option<DropLanding> {
    let (target, landing) = match aim {
        seats::LayoutAim::Rim(edge) => return Some(DropLanding::RootRim { edge }),
        seats::LayoutAim::SeatEdge(target, edge) => {
            (target, DropLanding::SeatEdge { target, edge })
        }
        seats::LayoutAim::SeatCentre(target) => (target, DropLanding::SeatCentre { target }),
    };
    (source != DragSource::Pane(target)).then_some(landing)
}

/// The mock-up's commit table (7202-7231), as one function.
///
/// **Three answers for five landings, and the grouping is the table's own.** A
/// strip reorder has already happened and is kept; the three landings inside the
/// layout are performed now; a tear-out and an empty hand are both "nowhere to
/// be".
///
/// **Why [`DropLanding::StripExtract`] sits with `None` and not with `Land`.**
/// N157's tear-out is the one landing here that ends with a pane in a *different
/// tab*, and a tab in this build is a tree plus exactly one shell — `TabState`
/// holds one `PtySession`, [`seats::Seats`] names one `terminal` seat, and
/// `Seats::to_persisted` takes one seed for the whole tree because (its own
/// words) "this window runs one shell per tab". Tearing a pane out therefore
/// produces either a tab with no shell or a tab with two, and I106 is the crash
/// report for the first of those: a pane that becomes a terminal with no session.
///
/// So it goes home — but *silently going home is not the whole answer*, and the
/// other half is [`Runtime::survey_strip`], which does not offer the landing at
/// all when the pair of tabs it would make is not one this build can host. M147
/// rules that a refusal must be visible, and a strip preview has no dashed form
/// to wear: the honest way for the strip to say "not this one" is the mock-up's
/// own, which is to draw nothing (6789's `paneCount > 1` guard is the same
/// sentence about a different limit). This arm is what remains after that — the
/// answer to a landing that cannot be surveyed today, kept because the *verdict*
/// is a fact about the landing rather than about which of them are reachable.
fn release_verdict(landing: Option<DropLanding>) -> DragRelease {
    match landing {
        Some(DropLanding::StripReorder { .. }) => DragRelease::Commit,
        Some(
            DropLanding::RootRim { .. }
            | DropLanding::SeatEdge { .. }
            | DropLanding::SeatCentre { .. },
        ) => DragRelease::Land,
        Some(DropLanding::StripExtract { .. }) | None => DragRelease::Home,
    }
}

/// **The tab model's own limit, stated once and asked in two places.**
///
/// A tab in this build is a tree *and a shell*: `TabState` carries one
/// `PtySession` and one `DualPlaneSession`, the frame pipeline publishes one
/// terminal frame per tab, and [`seats::Seats`] names a single `terminal` seat
/// that says where that shell draws. So a tree this window can hand to a tab is
/// one with exactly one terminal leaf in it. Two would be a pane drawn as an
/// empty box with no session behind it — the 2026-07-16 crash I106 is written
/// against — and none would be a tab with nothing running.
///
/// **This is a limit, not a ruling, and it is deliberately not phrased as one.**
/// Nothing in the mock-up forbids two shells in a tab; N159's merge and N161's
/// replace are *about* putting them there. What forbids it is that panes do not
/// own sessions yet, a gap `seats.rs`' own note names ("when panes get their own
/// children, this parameter becomes the per-leaf lookup"). Writing it here, as a
/// question about a tree rather than as a special case inside each gesture, is
/// what lets it be deleted in one place on the day that changes.
///
/// Until then it is asked where M147 can act on the answer: [`Runtime::plan_for`]
/// turns such a plan down so the promise on screen goes dashed instead of blue,
/// and [`Runtime::survey_strip`] declines to offer a tear-out that would make
/// one. A drop that cannot happen must not be drawn as one that can.
fn tab_can_host(tree: &LayoutNode) -> bool {
    tree.seats_in_order()
        .iter()
        .filter(|seat| seat.kind == bt_layout::SeatKind::Terminal)
        .count()
        == 1
}

/// A left press being held on a pane head (J118).
///
/// It holds the six pixels and nothing else, and the emptiness is the point: a
/// pane head press owes the pane nothing on the way in. D40's focus move has
/// already happened above the router by the time this is armed, and it happened
/// for *every* press inside the pane rather than for this one — so there is no
/// delayed promise to keep, nothing to withdraw when the hand travels, and
/// nothing left unpaid when a drag is cancelled.
#[derive(Clone, Copy, Debug)]
struct PanePress {
    seat: SeatId,
    latch: DragLatch,
}

impl TabState {
    /// The reveal as the strip would draw it: quantised to the 1/255 the
    /// sprite's opacity resolves to, which is the finest difference that can
    /// reach the screen.
    fn drawn_pin_reveal(&self, now: Instant, motion: Motion) -> u8 {
        let (reveal, _) = self.pin_reveal.sample(now, motion);
        (reveal.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    fn pin_is_animating(&self, now: Instant, motion: Motion) -> bool {
        self.pin_reveal.sample(now, motion).1
    }

    /// How far from its slot this tab is drawn, in physical pixels.
    ///
    /// Two sources, never both: a tab in the hand is wherever the pointer has
    /// put it, and every other tab is wherever its slide home has got to. Passing
    /// the carried offset in rather than reading the window's drag here is what
    /// keeps this a method on a tab instead of a method on the window.
    fn drawn_offset(&self, now: Instant, motion: Motion, grabbed: Option<f32>) -> f32 {
        match grabbed {
            Some(offset) => offset,
            None => self.flip.sample(now, motion).0,
        }
    }

    /// The facts this tab's session is reporting right now.
    fn session_facts(&self, tab_is_active: bool) -> SessionFacts {
        SessionFacts {
            status: self.session.status(),
            last_seen_revision: self.last_seen_revision,
            tab_is_active,
        }
    }

    /// Mark this tab as read, and retire the attention it had latched.
    ///
    /// Called when the tab becomes the one on screen. Looking at a tab is the
    /// event that answers every claim it was making: the output is now seen, the
    /// bell has been heard, and the failure has been read about. `bt-term` owns
    /// the two latches and exposes exactly one way to drop them, which is what
    /// keeps "the user looked" a single decision rather than three.
    fn mark_seen(&mut self) {
        self.last_seen_revision = self.session.published_revision();
        self.session.clear_attention();
    }

    /// Advance this tab's ring toward whatever its session is now reporting,
    /// and answer with what the strip should draw in its mark slot.
    ///
    /// The tween lives here rather than in the drawing code because it is
    /// *memory*: where the arc was when the reading changed. A pure function of
    /// the current status could only ever snap.
    fn mark_state(
        &self,
        tab_is_active: bool,
        now: Instant,
        motion: Motion,
        palette: &ChromePalette,
    ) -> seats::TabMarkState {
        let facts = self.session_facts(tab_is_active);
        let claim = loudest_claim([facts.claim()]);
        let ring = facts.status.progress.map(|state| {
            let arc = ring_arc(
                state,
                self.ring_sweep,
                self.animation_elapsed(now),
                motion,
                palette,
            );
            let sweep = match self.ring_tween {
                // A determinate arc eases; an indeterminate one is already
                // moving under its own animation and must not be eased on top
                // of it, or the spin would drag against the tween.
                Some(tween) if !arc.animating => tween.sample(now).0,
                _ => arc.sweep_milliturns,
            };
            seats::TabRing {
                arc: arc.color,
                start_milliturns: arc.start_milliturns,
                sweep_milliturns: sweep,
            }
        });
        seats::TabMarkState {
            dot: claim.dot_color(palette),
            ring,
            opacity: mark_opacity(
                facts.status.working,
                ring.is_some(),
                self.animation_elapsed(now),
                motion,
            ),
            // Prepared and unwired: nothing in this build can report a session
            // that has died while its tab lives on — see `reap_exited_tabs`,
            // which closes the tab the moment the PTY exits.
            grayscale: false,
        }
    }

    /// How long this tab's animations have been running.
    fn animation_elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.animation_epoch)
    }

    /// Whether anything in this tab's mark slot is moving under its own steam,
    /// and therefore owes the next frame.
    fn mark_is_animating(&self, now: Instant, motion: Motion) -> bool {
        if motion == Motion::Reduced {
            // Everything that moves has been stood down; only the tween
            // survives, and the mock-up's reduced-motion block leaves it alone.
            return self.ring_tween.is_some_and(|tween| tween.sample(now).1);
        }
        let status = self.session.status();
        status.working
            || matches!(status.progress, Some(ProgressState::Indeterminate))
            || self.ring_tween.is_some_and(|tween| tween.sample(now).1)
    }

    /// Notice a new progress reading and start the arc easing toward it.
    ///
    /// Returns whether anything changed, so the caller can tell a frame that is
    /// owed from one that is not.
    fn sync_ring(&mut self, now: Instant) {
        let Some(state) = self.session.status().progress else {
            // The run ended: the mark comes back and the ring's memory goes
            // with it, so the next run starts from nothing rather than from
            // wherever the last one stopped.
            self.ring_sweep = None;
            self.ring_tween = None;
            return;
        };
        let target = match state {
            ProgressState::Normal(percent) => Some(sweep_milliturns(percent)),
            ProgressState::Error(percent) | ProgressState::Paused(percent) => {
                percent.map(sweep_milliturns)
            }
            // An indeterminate arc has no reading to ease toward — its length
            // is fixed and its motion is the spin.
            ProgressState::Indeterminate => None,
        };
        let Some(target) = target else {
            return;
        };
        let showing = match self.ring_tween {
            Some(tween) => tween.sample(now).0,
            None => self.ring_sweep.unwrap_or(0),
        };
        if self.ring_sweep == Some(target) && self.ring_tween.is_none() {
            return;
        }
        if showing == target {
            self.ring_sweep = Some(target);
            self.ring_tween = None;
            return;
        }
        self.ring_sweep = Some(target);
        self.ring_tween = Some(SweepTween {
            from: showing,
            to: target,
            started: now,
        });
    }
}

#[derive(Debug)]
struct CursorBlink {
    focused: bool,
    visible: bool,
    next_toggle: Option<Instant>,
}

impl CursorBlink {
    fn new(now: Instant) -> Self {
        Self {
            focused: true,
            visible: true,
            next_toggle: Some(now + CURSOR_BLINK_PHASE),
        }
    }

    fn visible(&self) -> bool {
        self.visible
    }

    fn reset(&mut self, now: Instant) -> bool {
        let changed = !self.visible;
        self.visible = true;
        self.next_toggle = self.focused.then_some(now + CURSOR_BLINK_PHASE);
        changed
    }

    fn set_focused(&mut self, focused: bool, now: Instant) -> bool {
        self.focused = focused;
        self.reset(now)
    }

    fn advance(&mut self, now: Instant) -> bool {
        let Some(mut deadline) = self.next_toggle else {
            return false;
        };
        if now < deadline {
            return false;
        }
        let before = self.visible;
        while deadline <= now {
            self.visible = !self.visible;
            deadline += CURSOR_BLINK_PHASE;
        }
        self.next_toggle = Some(deadline);
        self.visible != before
    }

    fn deadline(&self) -> Option<Instant> {
        self.next_toggle
    }
}

fn focus_leaf_index(seats: &seats::Seats) -> usize {
    seats
        .tree()
        .seats_in_order()
        .iter()
        .position(|seat| seat.id == seats.focus())
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabCloseAction {
    CloseWindow,
    Keep { active_tab: usize },
}

fn tab_close_action(tab_count: usize, active_tab: usize, closing: usize) -> TabCloseAction {
    debug_assert!(tab_count > 0 && active_tab < tab_count && closing < tab_count);
    if tab_count == 1 {
        return TabCloseAction::CloseWindow;
    }
    let active_tab = if closing == active_tab {
        closing.min(tab_count - 2)
    } else if closing < active_tab {
        active_tab - 1
    } else {
        active_tab
    };
    TabCloseAction::Keep { active_tab }
}

/// How launch divides the last session: what opens now, and what it has to ask
/// about.
///
/// "Launch asks about exactly one thing, and it is not the pinned tabs.
/// **Pinning IS the answer**" (mock-up 7426-7431). §8.0 at the UI layer: do not
/// ask what you already know; ask only what you do not.
#[derive(Clone, Debug, Default, PartialEq)]
struct LaunchPlan {
    /// Revived before the window is usable, in this order.
    open: Vec<TabV1>,
    /// The restore prompt's question. Empty means no prompt at all.
    ask: Vec<TabV1>,
    /// Which entry of `open` was the tab you were last on, when that tab is one
    /// of the ones coming back now. `None` leaves the choice to the first tab —
    /// restoring must not steal the seat from a pinned tab that is already up.
    active_open: Option<usize>,
    /// Whether the window needs a placeholder shell because nothing was pinned.
    /// §7.1.4: "无 pinned 可恢复时以默认 profile 的占位 shell 起步, restore 接受后
    /// 占位（未被使用时）移除".
    placeholder: bool,
}

fn plan_launch(saved: &[TabV1], active: usize) -> LaunchPlan {
    let mut open = Vec::new();
    let mut ask = Vec::new();
    let mut active_open = None;
    for (index, tab) in saved.iter().enumerate() {
        if tab.pinned {
            if index == active {
                active_open = Some(open.len());
            }
            open.push(tab.clone());
        } else {
            ask.push(tab.clone());
        }
    }
    // The one-tab boundary, ruled here rather than left to the prompt: when
    // nothing was pinned and exactly one tab was open, "Reopen your **other**
    // tabs?" has no other tabs to name, and declining would hand back a fresh
    // shell in the wrong folder — a strictly worse version of the same single
    // tab. There is no question to ask, so we do not ask one.
    if open.is_empty() && ask.len() == 1 {
        return LaunchPlan {
            open: ask,
            ask: Vec::new(),
            active_open: Some(0),
            placeholder: false,
        };
    }
    LaunchPlan {
        placeholder: open.is_empty(),
        open,
        ask,
        active_open,
    }
}

/// The seed a persisted tab comes back as: its tree, the three facts it was
/// started from, and the folder to stand its new shell in.
///
/// One function, because a pinned tab at launch, a Restore, a Recent row and
/// Ctrl+Shift+T must all produce the *same* tab from the same bytes — "if this
/// had its own revive path the three would drift, and the one that drifts is
/// always the one you use least" (mock-up 7347-7350).
fn revive_plan(tab: &TabV1) -> (seats::Seats, TabSeed, Option<PathBuf>) {
    let mut seats =
        seats::Seats::from_persisted(&tab.root).unwrap_or_else(seats::Seats::lone_terminal);
    seats.restore_focus_token(&tab.focused_leaf);
    let leaf = first_term_leaf(&tab.root);
    let seed = TabSeed {
        profile: leaf
            .map(|leaf| profiles::index_of_id(&leaf.profile_id))
            .unwrap_or(profiles::DEFAULT_PROFILE),
        manual_name: leaf.and_then(|leaf| leaf.manual_name.clone()),
        pinned: tab.pinned,
    };
    // An empty `cwd` is a shell that never reported one, not a path to the root
    // of the drive — hand over nothing and let the new shell start where a fresh
    // one would. Whether the folder still exists is a filesystem question, and
    // the answer to a missing one is the same as the answer to none: HOME.
    let cwd = leaf
        .map(|leaf| leaf.cwd.as_str())
        .filter(|cwd| !cwd.is_empty())
        .map(PathBuf::from)
        .filter(|cwd| cwd.is_dir());
    (seats, seed, cwd)
}

/// How many panes a persisted tab held — the number its badge would show.
fn persisted_pane_count(node: &LayoutNodeV1) -> usize {
    match node {
        LayoutNodeV1::Leaf(_) => 1,
        LayoutNodeV1::Split(split) => split.children.iter().map(|c| persisted_pane_count(c)).sum(),
    }
}

/// The first terminal leaf of a persisted tree, in the order the tree is drawn.
/// A tab's identity is the terminal it holds; a files-only tab has none, and
/// seeds as a default shell rather than refusing to come back at all.
fn first_term_leaf(node: &LayoutNodeV1) -> Option<&TermLeafV1> {
    match node {
        LayoutNodeV1::Leaf(LeafNodeV1::Term(leaf)) => Some(leaf),
        LayoutNodeV1::Leaf(_) => None,
        LayoutNodeV1::Split(split) => split
            .children
            .iter()
            .find_map(|child| first_term_leaf(child)),
    }
}

/// What a tab is *started from* — the three facts a [`seed::Seed`] carries, plus
/// whether the tab arrives already pinned.
///
/// Every door into the tab machinery goes through this: a fresh `+`, a profile
/// row, a Recent entry, an undo-close and a pinned tab revived at launch all
/// hand over the same shape. That is the whole point of "one vault, three doors"
/// expressed in the type system — a door that could not say where its tab stood
/// would be a door that opened somewhere else.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TabSeed {
    /// Index into [`profiles::PROFILES`].
    profile: usize,
    /// The user's name for the tab, if it had one. It survives a close because
    /// it is the one layer of the name nobody else can regenerate.
    manual_name: Option<String>,
    pinned: bool,
}

impl TabSeed {
    /// A tab the user is starting now: the profile they picked, no name yet, and
    /// not pinned — pinning is a promise about *next* launch and nobody has made
    /// it yet.
    fn of_profile(profile: usize) -> Self {
        Self {
            profile,
            ..Self::default()
        }
    }
}

// Eight, and every one of them is a different question the answer to which
// cannot be derived from the others: identity, shape, the two rendering
// facts, the wake channel, the probe bytes, the place, and the seed. Bundling
/// Spawn one shell for one Terminal leaf, sized to the rectangle it will draw
/// into.
///
/// Factored out of [`create_tab_state`] because a tab's first shell and a
/// split's second one are the same act: the only difference is which seat they
/// are filed under and which directory they open in. Keeping one builder is what
/// stops a pane born from a split from quietly differing — a missing quota, an
/// unset baseline, a layout key nobody seeded — from a pane born with its tab.
fn create_leaf_session(
    renderer: &Renderer,
    body: bt_render::SeatViewport,
    wake: OutputWake,
    probe_input: Option<&[u8]>,
    working_directory: Option<PathBuf>,
) -> Result<LeafSession> {
    let grid = renderer.metrics().grid_for_pixels(body.width, body.height);
    let mut pty = if probe_input.is_none() {
        Some(
            PtySession::spawn_default_in(
                pty_size(grid, PhysicalSize::new(body.width, body.height)),
                wake,
                working_directory,
            )
            .context("spawn default PowerShell in ConPTY")?,
        )
    } else {
        None
    };
    let shell_fallback_notice = pty
        .as_mut()
        .and_then(PtySession::take_shell_fallback_notice);
    let columns = nonzero_u32(grid.columns.get());
    let rows = nonzero_u32(grid.rows.get());
    let mut session = DualPlaneSession::with_quotas_and_cell_height(
        columns,
        rows,
        DEFAULT_STAGING_QUOTA,
        M0_FROZEN_LINE_QUOTA,
        renderer.metrics().cell_height_subpixels(),
    );
    session.set_cell_width_subpixels(cell_width_subpixels(renderer.metrics()));
    session.set_ascii_baseline_subpixels(renderer.metrics().ascii_baseline_subpixels());
    session.set_math_layout_options(MathLayoutOptions {
        detect_image_paths: true,
        ..MathLayoutOptions::default()
    });
    session.set_layout_key(LayoutKey {
        width_cells: columns,
        dpi_milli: renderer.metrics().dpi_milli(),
        font_rev: 1,
        theme_rev: theme_revision(),
    });
    if let Some(bytes) = probe_input {
        session
            .feed(bytes)
            .context("feed BT_PROBE_INPUT bytes directly into terminal")?;
    }
    let projection = session.new_projection(session.layout_key());
    Ok(LeafSession {
        pty,
        session,
        shell_fallback_notice,
        projection,
        grid,
        conpty_grid: grid,
        pending_pty_resize: None,
        pending_psreadline_resize_reanchor: false,
        last_seen_revision: 0,
        last_presented_frame: None,
    })
}

// them into a struct would move the argument list rather than shorten it.
#[allow(clippy::too_many_arguments)]
fn create_tab_state(
    id: TabId,
    seats: seats::Seats,
    renderer: &Renderer,
    render_physical: PhysicalSize<u32>,
    wake: OutputWake,
    probe_input: Option<&[u8]>,
    working_directory: Option<PathBuf>,
    seed: TabSeed,
) -> Result<(TabState, String)> {
    let (seat_layout, seat_overflow, terminal_seat, _) =
        solve_seats(&seats, renderer, render_physical);
    // Captured before `seats` moves into the tab: the seat this first shell
    // draws into is the key its session is filed under.
    let terminal_seat_id = seats.terminal();
    let leaf = create_leaf_session(
        renderer,
        terminal_seat,
        wake,
        probe_input,
        working_directory,
    )?;
    let conpty_source = leaf
        .pty
        .as_ref()
        .map(|pty| pty.conpty_source().to_string())
        .unwrap_or_else(|| "direct-input".to_string());
    Ok((
        TabState {
            id,
            sessions: BTreeMap::from([(terminal_seat_id, leaf)]),
            focused_leaf: terminal_seat_id,
            profile: seed.profile,
            pinned: seed.pinned,
            manual_name: seed.manual_name,
            pending_keyboard_at: None,
            // A tab that arrives pinned wears its pin from the first frame; it
            // is a fact about the tab, not an offer that has to be hovered out.
            pin_reveal: RevealTween {
                from: f32::from(u8::from(seed.pinned)),
                to: f32::from(u8::from(seed.pinned)),
                started: None,
                span: Duration::from_millis(WINDOW_TAB_PIN_REVEAL_MS),
            },
            last_drawn_pin_reveal: None,
            ring_tween: None,
            ring_sweep: None,
            last_drawn_mark: None,
            animation_epoch: Instant::now(),
            flip: FlipTween::default(),
            landing: LandTween::default(),
            last_drawn_offset: None,
            last_drawn_landing: None,
            pending_resize_present: None,
            seats,
            seat_layout,
            seat_overflow,
            preview_image: None,
        },
        conpty_source,
    ))
}

/// Drain one shell's pipe into its own screen.
///
/// Returns whether anything arrived, and whether this shell's OSC 2 title
/// changed — the two facts the caller needs to decide what to redraw and what to
/// relabel.
fn drain_leaf_pty(leaf: &mut LeafSession) -> Result<(bool, bool)> {
    if leaf.pty.is_none() {
        return Ok((false, false));
    }
    let title_before = leaf.session.window_title().map(str::to_owned);
    let mut changed = false;
    loop {
        let bytes = leaf
            .pty
            .as_ref()
            .expect("PTY mode checked above")
            .read_output();
        if bytes.is_empty() {
            break;
        }
        debug_assert!(bytes.len() <= bt_pty::TERM_READ_QUANTUM.get());
        leaf.session
            .feed_at(&bytes, Instant::now())
            .context("apply PTY output")?;
        for reply in leaf.session.take_pty_writes() {
            leaf.pty
                .as_mut()
                .expect("PTY mode checked above")
                .write(&reply)
                .context("return terminal protocol reply to PTY")?;
        }
        changed = true;
    }
    Ok((
        changed,
        leaf.session.window_title() != title_before.as_deref(),
    ))
}

/// Drain every shell this tab holds.
///
/// Written as a loop rather than left to the tab's deref because "drain this
/// tab" has always meant *all of it*: a background pane that stops being read
/// fills its pipe and blocks the shell writing into it. The tab's answer is the
/// union of its leaves' — anything arrived anywhere, any title moved anywhere.
fn drain_tab_pty(tab: &mut TabState) -> Result<(bool, bool)> {
    let mut changed = false;
    let mut title_changed = false;
    for (_, leaf) in tab.leaves_mut() {
        let (leaf_changed, leaf_title_changed) = drain_leaf_pty(leaf)?;
        changed |= leaf_changed;
        title_changed |= leaf_title_changed;
    }
    Ok((changed, title_changed))
}

impl Runtime {
    fn create(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<AppEvent>,
        startup_started: Instant,
    ) -> Result<Self> {
        let trace_startup = std::env::var_os("BT_STARTUP_TRACE").is_some();
        let trace_resize = std::env::var_os("BT_RESIZE_TRACE").is_some();
        let trace_layout_events = std::env::var_os("BT_LAYOUT_EVENTS").is_some();
        let trace_perf = std::env::var_os("BT_PERF_TRACE").is_some();
        let phase_started = Instant::now();
        // Read the previous session before the window exists, so its bounds can
        // be the window's opening bounds rather than a correction applied after
        // the user has already seen it somewhere else.
        let session_store = persist::SessionStore::open();
        let theme_mode = render_theme_mode(session_store.loaded().theme);
        set_cursor_style(render_cursor_style(session_store.loaded().cursor_style));
        let restored = restore_window_placement(event_loop, session_store.loaded());
        let attributes = Window::default_attributes()
            .with_title(DEFAULT_PROFILE_TITLE)
            // Approximate on purpose: winit sizes by client area and the frame
            // installed below turns the client area into the whole outer rect, so
            // the exact rectangle can only be set once that frame exists. What
            // this opening size is for is landing the window on the right monitor
            // at the right DPI, which is what the exact one is then scaled by.
            .with_inner_size(
                restored
                    .map(|placement| placement.size)
                    .unwrap_or(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT)),
            )
            // Do not expose the system class brush while the first swapchain image is pending.
            .with_visible(false);
        let attributes = match restored.and_then(|placement| placement.position) {
            Some(position) => attributes.with_position(position),
            None => attributes,
        };
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create native window")?,
        );
        let resolved_theme = resolve_theme_mode(theme_mode, window.theme());
        if set_theme(resolved_theme) == ThemeChange::LockedByEnvironment {
            eprintln!(
                "BT_THEME persisted_mode={theme_mode:?} resolved_theme={resolved_theme:?} ignored_for_runtime=true reason=BT_BG"
            );
        }
        install_theme_class_background(&window)?;
        window.set_ime_allowed(true);
        let hwnd = window_hwnd(&window)?;
        let custom_window_frame = bt_platform::CustomWindowFrame::install(hwnd)
            .map_err(|error| anyhow!(error))
            .context("install self-drawn Win32 window frame")?;
        let ime_system_caret = bt_platform::ImeSystemCaret::new(hwnd);
        let math_context_menu = bt_platform::MathContextMenu::new(hwnd)
            .map_err(|error| anyhow!(error))
            .context("install deferred formula context menu")?;
        // Only now, with the self-drawn frame owning WM_NCCALCSIZE, does "the
        // window's rectangle" mean one thing instead of two. Restate the geometry
        // as that one rectangle: what winit built is the saved size plus a native
        // frame margin (26x71 physical at 192 DPI) that this window does not
        // wear, and left alone it would be saved, re-inflated and re-saved on
        // every restart. The window is still hidden, so this costs no flicker.
        let opened_at = dpi_snapshot(&window)?;
        bt_platform::set_window_outer_rect(
            hwnd,
            startup_window_rect(restored, opened_at.rect, opened_at.authoritative_scale),
        )
        .map_err(|error| anyhow!(error))
        .context("restore the window's outer rectangle")?;
        let window_time = phase_started.elapsed();
        let startup_dpi = dpi_snapshot(&window)?;
        let physical = window.inner_size();
        let startup_scale_factor = startup_dpi.authoritative_scale;
        let phase_started = Instant::now();
        let mut renderer = pollster::block_on(Renderer::new(
            Arc::clone(&window),
            physical.width,
            physical.height,
            startup_scale_factor,
        ))
        .context("initialize wgpu renderer")?;
        ensure_metrics_match_authoritative_scale(
            renderer.metrics().scale_factor,
            startup_scale_factor,
        )?;
        trace_surface_size_clamp(
            trace_startup || trace_resize,
            "BT_STARTUP",
            physical,
            renderer.presentation_geometry(),
        );
        ensure_swapchain_matches_inner(&renderer, physical)?;
        log_dpi_snapshot(
            "create",
            startup_dpi,
            None,
            renderer.presentation_geometry(),
            physical,
        );
        let renderer_time = phase_started.elapsed();
        let render_physical = presentation_physical_size(renderer.presentation_geometry());
        // The seam of §4.2, in the one order it is allowed to run: window
        // geometry -> viewport -> solve -> seat rects -> the terminal seat's
        // cols/rows. The persisted tree is layout *intent* (L11); the rectangle
        // it produces here is computed fresh against this machine's DPI.
        let probe_input = load_probe_input()?;
        let pty_proxy = proxy.clone();
        let wake: OutputWake = Arc::new(move || {
            let _ = pty_proxy.send_event(AppEvent::PtyOutput);
        });
        let phase_started = Instant::now();
        // Pinned tabs are an answer already given, so they simply open; the rest
        // become a question the prompt will ask over a window that already works.
        let plan = if probe_input.is_some() {
            LaunchPlan::default()
        } else {
            let loaded = session_store.loaded();
            plan_launch(&loaded.tabs, loaded.active_tab as usize)
        };
        let restored_roots: Vec<_> = if plan.open.is_empty() {
            vec![(seats::Seats::lone_terminal(), TabSeed::default(), None)]
        } else {
            plan.open.iter().map(revive_plan).collect()
        };
        let mut tabs = Vec::with_capacity(restored_roots.len());
        let mut conpty_sources = Vec::with_capacity(restored_roots.len());
        for (index, (seats, seed, working_directory)) in restored_roots.into_iter().enumerate() {
            let (tab, conpty_source) = create_tab_state(
                TabId(index as u64 + 1),
                seats,
                &renderer,
                render_physical,
                Arc::clone(&wake),
                if index == 0 {
                    probe_input.as_deref()
                } else {
                    None
                },
                // A revived tab stands where its seed says, not where some other
                // tab happens to be — that address IS what a seed is for.
                working_directory,
                seed,
            )?;
            tabs.push(tab);
            conpty_sources.push(conpty_source);
        }
        // The tab you were on comes back on top, if it was one of the ones that
        // came back. A placeholder shell is index 0 either way.
        let active_tab = plan.active_open.unwrap_or(0).min(tabs.len() - 1);
        let recent = seed::SeedVault::from_persisted(&session_store.loaded().recent);
        let pending_restore = plan.ask.clone();
        let has_question = !pending_restore.is_empty();
        // Only a shell we opened *because we had no answer* is scaffolding. A
        // lone restored tab is a tab, and must never be swept away by a later
        // Restore.
        let placeholder_tab = plan.placeholder.then(|| tabs[0].id);
        let (_, _, terminal_seat, seat_viewport) =
            solve_seats(&tabs[active_tab].seats, &renderer, render_physical);
        renderer.set_seat_viewport(terminal_seat);
        if trace_startup || trace_resize {
            eprintln!("BT_CONPTY_SOURCE sources={conpty_sources:?}");
        }
        let pty_time = phase_started.elapsed();
        let math_worker = MathWorker::spawn(proxy.clone())?;
        let mut runtime = Self {
            renderer,
            tabs,
            active_tab,
            next_tab_id: conpty_sources.len() as u64 + 1,
            event_proxy: proxy.clone(),
            math_worker,
            math_worker_running: true,
            math_worker_notice_pending: false,
            pending_frames: LatestFrameSlot::default(),
            modifiers: ModifiersState::default(),
            math_context_menu,
            custom_window_frame,
            window,
            startup_started,
            trace_startup,
            trace_resize,
            trace_layout_events,
            last_layout_events: Vec::new(),
            trace_perf,
            resize_trace_logged_transaction: 0,
            resize_trace_logged_events: 0,
            background_visible: None,
            first_text_visible: None,
            window_shown: false,
            first_visible_present_dpi_checked: false,
            first_text_presented: false,
            last_presented_frame: None,
            preedit: None,
            ime_active: false,
            ime_cursor_throttle: ImeCursorThrottle::default(),
            cursor_blink: CursorBlink::new(Instant::now()),
            motion: read_motion_preference(),
            // A window is focused when it opens, and `CursorBlink` starts from
            // the same assumption — the two must agree or the strip and the
            // caret would disagree about whether anyone is home.
            window_focused: true,
            ime_system_caret,
            pointer_position: None,
            mouse_route: None,
            click_tracker: ClickTracker::default(),
            line_wheel_remainder: 0.0,
            pixel_wheel_remainder: 0.0,
            notch_wheel_remainder: 0.0,
            local_wheel_subpixel_remainder: 0.0,
            hyperlink_hover: HyperlinkHover::default(),
            frame_image_references: FrameImageReferences::default(),
            underlined_image_reference: None,
            peek_hover: PeekHover::default(),
            peek_cache: std::collections::HashMap::new(),
            peek_thumbnail: None,
            peek_thumbnail_pending: None,
            math_hover_anchor: None,
            math_hover_clear_at: None,
            pending_math_context_anchor: None,
            seat_pointer: seats::ChromePointer::default(),
            tooltip: tooltip::TooltipHost::default(),
            layout_peek: peek_strip::PeekHost::default(),
            tooltip_anchors: tooltip::TooltipAnchors::default(),
            tooltip_drawn_opacity: None,
            chrome_marks: marks::ChromeMarkRasters::default(),
            settings: settings::SettingsPanel::default(),
            profile_menu: profiles::ProfileMenu::default(),
            chevron_turn: ChevronTurn::default(),
            last_drawn_chevron: None,
            tab_scroll: 0.0,
            tab_press: None,
            pane_press: None,
            drag: None,
            drop_preview: None,
            last_drawn_dock_reveal: None,
            seat_viewport,
            tab_clicks: TabClicks::default(),
            rename: None,
            rename_blink: CursorBlink::new(Instant::now()),
            settings_marks: marks::ChromeMarkRasters::default(),
            divider_drag: None,
            work_area: WorkAreaHint::NeverKnown,
            session_store,
            recent,
            pending_restore,
            // "It opens BEFORE it asks — like a browser, which lands you on
            // your pages and puts 'restore?' on top of a window that already
            // works" (mock-up 7435-7439). The window is built by the time this
            // runs, so the question arrives over something usable.
            restore_prompt: {
                let mut prompt = restore::RestorePrompt::default();
                if has_question {
                    prompt.open();
                }
                prompt
            },
            placeholder_tab,
            theme_mode,
            window_min_inner_size: None,
        };
        runtime.refresh_work_area();
        runtime.apply_window_min_inner_size()?;
        runtime.window.set_title(&runtime.display_title());
        runtime.refresh_chrome();
        if trace_startup {
            let renderer_phases = runtime.renderer.init_timings();
            eprintln!(
                "BT_STARTUP window={}ms adapter={}ms device={}ms surface={}ms fonts={}ms metrics={}ms render_resources={}ms renderer_total={}ms pty_spawn={}ms probe_input={} conpty_sources={conpty_sources:?} runtime_ready={}ms",
                window_time.as_millis(),
                renderer_phases.adapter.as_millis(),
                renderer_phases.device.as_millis(),
                renderer_phases.surface_configure.as_millis(),
                renderer_phases.font_system.as_millis(),
                renderer_phases.font_metrics.as_millis(),
                renderer_phases.render_resources.as_millis(),
                renderer_time.as_millis(),
                pty_time.as_millis(),
                probe_input.as_ref().map_or(0, Vec::len),
                startup_started.elapsed().as_millis(),
            );
        }
        runtime.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        // A hidden surface can either accept the clear or report Occluded. In the latter case
        // redraw() republishes the frame; the second call presents immediately after ShowWindow.
        runtime.redraw()?;
        // Maximize before the window is shown, not after: `SW_MAXIMIZE` reveals a
        // hidden window itself, so this is one transition into the state the
        // session recorded rather than a normal-sized window that jumps. The
        // normal rectangle set above survives as the placement Windows restores
        // the window to when the user unmaximizes it.
        if restored.is_some_and(|placement| placement.maximized) {
            runtime.window.set_maximized(true);
        }
        runtime.window.set_visible(true);
        runtime.window_shown = true;
        // Showing a hidden Win32 window can synchronously settle it onto a different monitor.
        // Query Win32 directly: winit's cached scale can race during initial monitor placement.
        runtime.reconcile_authoritative_dpi("show")?;
        // Force one presentation after ShowWindow so the first-present reconciliation below is a
        // visible startup stage even when the hidden pre-clear already succeeded.
        runtime.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        runtime.redraw()?;
        let background_visible = startup_started.elapsed();
        runtime.background_visible = Some(background_visible);
        if trace_startup {
            eprintln!(
                "BT_STARTUP background_visible={}ms",
                background_visible.as_millis()
            );
        }
        Ok(runtime)
    }

    /// The `+`'s verb: a tab on the default profile, which is what the button's
    /// own tooltip promises in the mock-up (`New tab (${defaultProfile().title})`).
    fn new_tab(&mut self) -> Result<()> {
        self.new_tab_with_profile(profiles::DEFAULT_PROFILE)
    }

    /// The picker's verb: a tab on the profile the row names.
    ///
    /// The parameter is the whole of the difference, and it is here rather than
    /// deeper because this build launches one shell: routing it further would be
    /// a parameter carried through three call frames to be ignored at the end of
    /// them. What the door has to be is *open* — one entry point that takes which
    /// profile, so a second profile is a launcher and not a new path through the
    /// tab machinery.
    fn new_tab_with_profile(&mut self, profile: usize) -> Result<()> {
        debug_assert!(profile < profiles::PROFILES.len());
        let render_physical = presentation_physical_size(self.renderer.presentation_geometry());
        let proxy = self.event_proxy.clone();
        let wake: OutputWake = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::PtyOutput);
        });
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        // I88 — "a new shell opens where the one you are looking at is standing"
        // (mock-up line 3961). The address is the focused session's own OSC 7
        // report, so a tab only inherits a directory its shell actually named;
        // a session that has never reported one hands over nothing and the new
        // shell starts where it always did.
        let inherited = self.session.working_directory().map(Path::to_path_buf);
        let (tab, _) = create_tab_state(
            id,
            seats::Seats::lone_terminal(),
            &self.renderer,
            render_physical,
            wake,
            None,
            inherited,
            TabSeed::of_profile(profile),
        )?;
        self.tabs.push(tab);
        self.apply_window_min_inner_size()?;
        self.activate_tab(self.tabs.len() - 1, true)
    }

    /// Scroll the strip until `index` is wholly on screen, and report whether
    /// anything moved.
    ///
    /// The verb that needs it is activation: a tab you have just switched to, or
    /// have just made, is the one thing in the strip that must not be off-screen.
    /// Everything else the strip does about scrolling, it does because the wheel
    /// asked.
    fn reveal_tab(&mut self, index: usize) -> bool {
        let scale = self.renderer.metrics().scale_factor as f32;
        let width = self.renderer.presentation_geometry().swapchain_size.0 as f32;
        let scrolled = seats::tab_scroll_to_reveal(
            width,
            scale,
            self.tabs.len(),
            self.active_tab,
            self.tab_scroll,
            index,
        );
        let moved = scrolled != self.tab_scroll;
        self.tab_scroll = scrolled;
        moved
    }

    fn activate_tab(&mut self, index: usize, force: bool) -> Result<()> {
        if index >= self.tabs.len() || (!force && index == self.active_tab) {
            return Ok(());
        }
        self.active_tab = index;
        // Looking at a tab is what answers every claim it was making, so the
        // dot goes out here — the unread mark, the bell and the failure all at
        // once, because "the user has now seen this tab" is one event and not
        // three. Ordered with the assignment above, not with the drawing below:
        // the strip is rebuilt from this state at the end of this function, and
        // a tab that became active while still counting as unread would flash
        // its own dot on the way in.
        self.tabs[index].mark_seen();
        // Ordered after the assignment on purpose: the tab being revealed is the
        // active one, and an active tab is measured with the skirt only an active
        // tab has.
        self.reveal_tab(index);
        let _ = self.pending_frames.take();
        self.last_presented_frame = None;
        self.preedit = None;
        self.mouse_route = None;
        self.divider_drag = None;
        self.seat_pointer = seats::ChromePointer::default();
        self.hyperlink_hover.clear();
        self.peek_hover.clear();
        self.renderer.set_peek_overlay(None);
        self.frame_image_references = FrameImageReferences::default();
        self.underlined_image_reference = None;
        let render_physical = presentation_physical_size(self.renderer.presentation_geometry());
        let next_grid = self.resolve_seat_layout(render_physical);
        self.schedule_grid_change(
            next_grid,
            terminal_pty_physical(&self.renderer, render_physical),
            Instant::now(),
            "resize activated tab to its seat layout",
        )?;
        self.sync_math_layout_key();
        self.window.set_title(&self.display_title());
        self.refresh_chrome();
        self.mark_session_dirty(Instant::now());
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    /// Pin or unpin, and put the strip back in order.
    ///
    /// Everything that changes a pin comes through here, so the "pinned lead"
    /// invariant has one enforcer instead of one per caller (mock-up 4066-4073).
    /// The active tab is followed by identity across the reorder: it is stored as
    /// a position, and a position means a different tab after a sort.
    fn toggle_pin(&mut self, index: usize) -> Result<()> {
        if index >= self.tabs.len() {
            return Ok(());
        }
        self.tabs[index].pinned = !self.tabs[index].pinned;
        let active = self.tabs[self.active_tab].id;
        seed::normalize_pins(&mut self.tabs, |tab| tab.pinned);
        self.active_tab = self
            .tabs
            .iter()
            .position(|tab| tab.id == active)
            .unwrap_or(self.active_tab.min(self.tabs.len() - 1));
        self.reveal_tab(self.active_tab);
        self.refresh_chrome();
        self.mark_session_dirty(Instant::now());
        self.present_chrome_change()
    }

    /// Open a Recent entry as a new tab — the door Recent and Ctrl+Shift+T share.
    ///
    /// Index 0 is "the one I just closed", which is the whole of what undo-close
    /// is: not a separate store, just the front of this one.
    fn reopen_recent(&mut self, index: usize) -> Result<()> {
        let Some(seed) = self.recent.take(index) else {
            return Ok(());
        };
        let seed::Seed::Term {
            profile_id,
            cwd,
            manual_name,
        } = seed
        else {
            // A files place has no shell to start; the pane that would host it
            // is T5's, and until then such an entry cannot be written either.
            return Ok(());
        };
        let render_physical = presentation_physical_size(self.renderer.presentation_geometry());
        let proxy = self.event_proxy.clone();
        let wake: OutputWake = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::PtyOutput);
        });
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        let working_directory = Some(PathBuf::from(&cwd)).filter(|cwd| cwd.is_dir());
        let (tab, _) = create_tab_state(
            id,
            seats::Seats::lone_terminal(),
            &self.renderer,
            render_physical,
            wake,
            None,
            working_directory,
            TabSeed {
                profile: profiles::index_of_id(&profile_id),
                manual_name,
                // A reopened tab is not pinned: it is coming back because you
                // asked for it now, which is not the same as promising to bring
                // it back every time.
                pinned: false,
            },
        )?;
        // Appended, which keeps the pinned run intact without a re-sort: a new
        // unpinned tab belongs at the end by construction.
        self.tabs.push(tab);
        self.apply_window_min_inner_size()?;
        self.activate_tab(self.tabs.len() - 1, true)
    }

    /// Answer the restore prompt.
    ///
    /// Restoring **appends** — the pinned tabs are already standing and are not
    /// up for discussion (mock-up 7492-7496). Declining keeps whatever you are
    /// looking at, which is why the button says "No thanks" rather than "Start
    /// fresh": fresh is already on the screen.
    ///
    /// Declining is not discarding. The tabs you did not take back go into the
    /// vault, so the door you did not walk through is still there — Ctrl+Shift+T
    /// and the Recent list can both still reach them. Nothing a user had open is
    /// ever dropped on the floor by a single click.
    fn answer_restore(&mut self, restore: bool) -> Result<()> {
        let pending = std::mem::take(&mut self.pending_restore);
        if pending.is_empty() {
            return Ok(());
        }
        if !restore {
            let now = SystemTime::now();
            for tab in &pending {
                if let Some(leaf) = first_term_leaf(&tab.root) {
                    self.recent.record(
                        seed::Seed::Term {
                            profile_id: leaf.profile_id.clone(),
                            cwd: leaf.cwd.clone(),
                            manual_name: leaf.manual_name.clone(),
                        },
                        now,
                    );
                }
            }
            self.mark_session_dirty(Instant::now());
            return Ok(());
        }
        let render_physical = presentation_physical_size(self.renderer.presentation_geometry());
        // The placeholder existed only because we had no answer; now we do. It
        // goes only if it is untouched — a shell you have already typed into is
        // yours, not scaffolding.
        let placeholder = self.placeholder_tab.take();
        let first_revived = self.tabs.len();
        for tab in &pending {
            let (seats, seed, working_directory) = revive_plan(tab);
            let proxy = self.event_proxy.clone();
            let wake: OutputWake = Arc::new(move || {
                let _ = proxy.send_event(AppEvent::PtyOutput);
            });
            let id = TabId(self.next_tab_id);
            self.next_tab_id += 1;
            let (revived, _) = create_tab_state(
                id,
                seats,
                &self.renderer,
                render_physical,
                wake,
                None,
                working_directory,
                seed,
            )?;
            self.tabs.push(revived);
        }
        if let Some(placeholder) = placeholder
            && self.tabs.len() > 1
            && let Some(index) = self.tabs.iter().position(|tab| tab.id == placeholder)
        {
            let mut removed = self.tabs.remove(index);
            if let Some(pty) = removed.pty.as_mut() {
                pty.shutdown().context("shut down the placeholder shell")?;
            }
        }
        self.apply_window_min_inner_size()?;
        let landing = first_revived.saturating_sub(usize::from(placeholder.is_some()));
        self.activate_tab(landing.min(self.tabs.len() - 1), true)
    }

    fn close_tab(&mut self, index: usize) -> Result<()> {
        if index >= self.tabs.len() {
            return Ok(());
        }
        // A tab that is going away takes its editor and its press with it. The
        // name is committed first rather than dropped, because closing is a blur
        // like any other and the seed the vault is about to record reads
        // `manual_name` — "输入到一半关掉,新名字进 Recent" is the same promise
        // §7.1.4 makes about closing the window.
        if self
            .rename
            .as_ref()
            .is_some_and(|editor| self.tabs.get(index).is_some_and(|tab| tab.id == editor.tab))
        {
            self.finish_rename(true)?;
        }
        if self
            .tab_press
            .is_some_and(|press| self.tabs.get(index).is_some_and(|tab| tab.id == press.tab))
        {
            self.tab_press = None;
        }
        if self.drag.is_some_and(|drag| {
            self.tabs
                .get(index)
                .is_some_and(|tab| drag.tab() == Some(tab.id))
        }) {
            self.drag = None;
        }
        match tab_close_action(self.tabs.len(), self.active_tab, index) {
            TabCloseAction::CloseWindow => {
                let hwnd = window_hwnd(&self.window)?;
                bt_platform::request_window_close(hwnd)
                    .map_err(|error| anyhow!(error))
                    .context("request close after the final tab")?;
            }
            TabCloseAction::Keep { active_tab } => {
                let was_active = index == self.active_tab;
                // The one regular write path into the vault: closing is what
                // fills Recent (mock-up 3929). It happens before the tab is
                // taken apart, because the seed is read off the live session.
                self.recent
                    .record(self.tabs[index].seed(), SystemTime::now());
                let mut removed = self.tabs.remove(index);
                if let Some(pty) = removed.pty.as_mut() {
                    pty.shutdown()
                        .context("shut down closed tab child process")?;
                }
                self.active_tab = active_tab;
                self.apply_window_min_inner_size()?;
                if was_active {
                    self.activate_tab(active_tab, true)?;
                } else {
                    self.refresh_chrome();
                    self.mark_session_dirty(Instant::now());
                    self.present_chrome_change()?;
                }
            }
        }
        Ok(())
    }

    /// Re-solve the tree against the current surface and place the terminal
    /// seat. Returns the grid the terminal seat's rectangle asks for.
    ///
    /// This is the only place cols/rows are derived from pixels once seats
    /// exist, and it derives them from the *seat's* rectangle rather than the
    /// window's. The direction is one-way (red line L10): what comes back out
    /// of the terminal never re-enters here.
    fn resolve_seat_layout(&mut self, render_physical: PhysicalSize<u32>) -> GridSize {
        let (layout, overflow, terminal_seat, viewport) =
            solve_seats(&self.seats, &self.renderer, render_physical);
        self.seat_viewport = viewport;
        // T230, and the reason the diff is taken *here*: this is the one place a
        // solved layout becomes the layout, so it is the one place that can tell
        // a real change from a rebuild that landed on the same answer. Every
        // cause §3.5 lists — the ladder, a divider drag, a swap, focus mode —
        // passes through it, and none of them has to remember to say so.
        let events = seats::layout_events(&self.seat_layout, &layout);
        self.seat_layout = layout;
        self.seat_overflow = overflow;
        self.publish_layout_events(events);
        self.renderer.set_seat_viewport(terminal_seat);
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.renderer
            .metrics()
            .grid_for_pixels(terminal_seat.width, terminal_seat.height)
    }

    /// Hand one commit's geometry changes to whoever is listening (T230).
    ///
    /// See [`Runtime::last_layout_events`] for who that is, and is not, today.
    fn publish_layout_events(&mut self, events: Vec<seats::LayoutEvent>) {
        self.last_layout_events = events;
        if self.trace_layout_events && !self.last_layout_events.is_empty() {
            eprintln!("BT_LAYOUT_EVENTS {:?}", self.last_layout_events);
        }
    }

    fn seat_metrics(&self) -> SeatMetrics {
        seats::seat_metrics(self.renderer.metrics().dpi_milli().get())
    }

    /// Rebuild the chrome quads and labels from the current solve. Returns
    /// whether anything visible changed.
    fn refresh_chrome(&mut self) -> bool {
        let scale = self.renderer.metrics().scale_factor as f32;
        let (width, _) = self.renderer.presentation_geometry().swapchain_size;
        self.custom_window_frame
            .set_tab_strip_right_px(seats::tab_strip_right_px(
                width as f32,
                scale,
                self.tabs.len(),
            ));
        // The badge's box is a function of the number in it, and only the font
        // knows how wide a number is — so the measuring happens here, where the
        // renderer is, and the strip is handed the answer rather than a font.
        let now = Instant::now();
        let palette = bt_render::chrome_palette();
        let renaming = self.rename.as_ref().map(|editor| editor.tab);
        // Only a tab drag lifts a tab out of the strip; a pane in the air leaves
        // the strip exactly as it was.
        let carried = self
            .drag
            .and_then(|drag| drag.tab_carry().map(|carry| carry.offset));
        let grabbed = self.drag.and_then(|drag| {
            let tab = drag.tab()?;
            self.tabs.iter().position(|candidate| candidate.id == tab)
        });
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let pane_count = tab.seats.pane_count();
                (
                    tab.display_title(),
                    pane_count,
                    tab.mark_state(index == self.active_tab, now, self.motion, &palette),
                    seats::TabTrailer {
                        pinned: tab.pinned,
                        reveal: tab.pin_reveal.sample(now, self.motion).0,
                    },
                    tab.drawn_offset(now, self.motion, carried.filter(|_| grabbed == Some(index))),
                    tab.landing.sample(now, self.motion).0,
                    // The layer under the override, which is exactly what the
                    // editor's placeholder shows: `autoName(s)` is `displayName`
                    // with the manual name taken out (mock-up 2605-2606).
                    (renaming == Some(tab.id)).then(|| {
                        display_title(
                            None,
                            tab.session.window_title(),
                            tab.session.working_directory(),
                        )
                    }),
                )
            })
            .collect::<Vec<_>>();
        let badge_font_px = bt_render::WINDOW_TAB_BADGE_FONT_LOGICAL_PX * scale;
        let mut tabs = tabs
            .into_iter()
            .map(
                |(title, pane_count, mark, trailer, offset, landing, placeholder)| {
                    seats::TabContent {
                        badge_text_width: if pane_count > 1 {
                            self.renderer
                                .measure_chrome_text(&pane_count.to_string(), badge_font_px)
                        } else {
                            0.0
                        },
                        // Filled in below, once the strip's own geometry has said how
                        // much room the box has: the editor scrolls to keep its caret
                        // in sight, and "in sight" is a width this loop has not
                        // computed yet.
                        edit: placeholder.map(|placeholder| seats::TabEdit {
                            placeholder,
                            ..seats::TabEdit::default()
                        }),
                        title,
                        pane_count,
                        mark,
                        trailer,
                        offset,
                        landing,
                    }
                },
            )
            .collect::<Vec<_>>();
        self.measure_open_rename(&mut tabs, scale, width as f32);
        // **K124 — the stand-in goes into the run.** Inserted after the rename
        // editor has been measured, because the editor is a fact about a real tab
        // and the indices it was measured against are the strip's own; the
        // stand-in is a guest that takes a slot for one gesture and then leaves.
        let mut active_tab = self.active_tab;
        let mut grabbed = grabbed;
        let mut strip_preview = None;
        if let Some((slot, stand_in)) = self.strip_stand_in() {
            tabs.insert(slot, stand_in);
            strip_preview = Some(slot);
            // Everything the strip indexes by position moves over with it. A
            // stand-in inserted before the active tab does not make its
            // *neighbour* active, and neither does it hand the grab to someone
            // else.
            active_tab += usize::from(active_tab >= slot);
            grabbed = grabbed.map(|index| index + usize::from(index >= slot));
        }
        let preview_title = self.preview_image.as_ref().map(PreviewImageState::title);
        // C28: a terminal pane head names the place it is in, at full length.
        let terminal_cwd = self
            .session
            .working_directory()
            .map(|path| path.to_string_lossy().into_owned());
        let preview_message = match self.preview_image.as_ref() {
            Some(preview) => preview.message(),
            // An open pane with nothing chosen invites rather than sits mute.
            None => self
                .seats
                .preview()
                .is_some()
                .then(|| "Click a dotted path to preview it here".to_owned()),
        };
        let (quads, labels, sprites) = seats::build_chrome_for_tabs(
            &self.seats,
            &self.seat_layout,
            scale,
            // E53's `body.dragging` is derived here rather than mirrored into a
            // field at every place a drag starts and ends: it is a fact about
            // the runtime, and the way for a mirror of it to go wrong is for one
            // of those places to be added later and forget.
            seats::ChromePointer {
                other_drag_in_flight: self.drag.is_some(),
                ..self.seat_pointer
            },
            seats::ChromeContent {
                tabs: &tabs,
                active_tab,
                grabbed,
                strip_preview,
                tab_scroll: self.tab_scroll,
                preview_title: preview_title.as_deref(),
                terminal_cwd: terminal_cwd.as_deref(),
                preview_message: preview_message.as_deref(),
                fit_overflow: self.seat_overflow,
                profile_menu_open: self.profile_menu.is_open(),
                chevron_turn: self.chevron_turn.sample(now, self.motion).0,
            },
        );
        let icons = self.chrome_marks.resolve(&sprites);
        let chrome_changed = self.renderer.set_chrome(quads, labels, icons);
        // From the same geometry, on the same beat: what the strip draws is what
        // can be tipped, and both are decided here or neither is.
        self.rebuild_tooltip_anchors(scale, width as f32, now);
        // The overlay is rebuilt from the same choke point as the chrome under
        // it, so every path that already knew to repaint on a resize, a DPI
        // change or a theme switch carries the dialog with it for free.
        let overlay_changed = self.refresh_overlay();
        chrome_changed || overlay_changed
    }

    /// Rebuild the tooltip's anchor list from the geometry the strip is about to
    /// be drawn with.
    ///
    /// Beside the chrome and from the same numbers, so an anchor cannot describe
    /// a box the strip is not drawing. Order is innermost-first, which is how
    /// [`tooltip::TooltipAnchors`] reproduces the mock-up's `closest()`: the pin
    /// and the mark answer before the tab they sit on.
    ///
    /// Registration is also where every suppression lands, because "do not tip
    /// this" and "this has nothing to say" are the same instruction to a host
    /// that only ever sees a list (M141).
    fn rebuild_tooltip_anchors(&mut self, scale: f32, width: f32, now: Instant) {
        let mut anchors = tooltip::TooltipAnchors::default();
        // A drag owns the pointer outright and everything else goes quiet for the
        // length of the gesture — the same rule hover, the peek flyout and the
        // terminal's own selection already live by. An empty list is how that is
        // said here: there is nothing to be over.
        if self.drag.is_none() {
            let geometry = seats::tab_strip_geometry(
                width,
                scale,
                &self.tab_trailers(now),
                self.active_tab,
                self.tab_scroll,
            );
            // What is cropped away is not there to be tipped, exactly as it is
            // not there to be clicked (`hit_tab_chrome`).
            let visible =
                |rect: [f32; 4]| rect[0] >= geometry.viewport[0] && rect[2] <= geometry.viewport[1];
            let renaming = self.rename.as_ref().map(|editor| editor.tab);
            for (index, slot) in geometry.tabs.iter().enumerate() {
                let Some(tab) = self.tabs.get(index) else {
                    continue;
                };
                // The editor IS the answer (mock-up 4193-4196): while you are
                // typing a name, a box telling you what the name currently is
                // would be covering the box you are typing it into.
                if renaming == Some(tab.id) {
                    continue;
                }
                if let Some(pin) = slot.pin.filter(|pin| visible(*pin)) {
                    anchors.push(
                        tooltip::TooltipAnchorId::TabPin(index),
                        pin,
                        if tab.pinned {
                            // Solid pin = "it is pinned", and the tip names the
                            // verb *and* the reason, because "Unpin" alone does
                            // not explain why the `×` went away (mock-up 4204).
                            "Unpin — a pinned tab closes only after unpinning"
                        } else {
                            "Pin"
                        },
                    );
                }
                let mark = seats::tab_mark_box(slot, scale);
                if visible(mark) {
                    let status = tab.session.status();
                    anchors.push(
                        tooltip::TooltipAnchorId::TabIcon(index),
                        mark,
                        tooltip::mark_tip(status.progress, status.working),
                    );
                }
                if visible(slot.body) {
                    anchors.push(
                        tooltip::TooltipAnchorId::Tab(index),
                        slot.body,
                        tab.tooltip_text(),
                    );
                }
                // The `×` is deliberately absent: `tabTrailer` writes no `title`
                // on it (mock-up 4207), so a pointer there falls through to the
                // tab — which is the tip you wanted anyway.
            }
            if visible(geometry.new_tab) {
                anchors.push(
                    tooltip::TooltipAnchorId::NewTab,
                    geometry.new_tab,
                    format!(
                        "New tab ({})",
                        profiles::PROFILES[profiles::DEFAULT_PROFILE].title
                    ),
                );
            }
            // I94: the chevron's own tip is silenced while its menu is up. You
            // just clicked it and the answer is on screen, so the question is
            // closed and the tip would be noise sitting on top of the answer.
            if visible(geometry.new_tab_menu) && !self.profile_menu.is_open() {
                anchors.push(
                    tooltip::TooltipAnchorId::NewTabMenu,
                    geometry.new_tab_menu,
                    "Choose a profile",
                );
            }
            for (target, rect) in seats::window_chrome_boxes(width, scale) {
                let text = match target {
                    // The gear, silenced while the dialog it opens is up — the
                    // chevron's rule, for the same reason.
                    seats::ChromeTarget::Settings if self.settings.is_open() => "",
                    seats::ChromeTarget::Settings => "Settings",
                    seats::ChromeTarget::Minimize => "Minimize",
                    seats::ChromeTarget::Maximize => "Maximize",
                    seats::ChromeTarget::CloseWindow => "Close",
                    _ => "",
                };
                let Some(id) = tooltip_anchor_for(target) else {
                    continue;
                };
                anchors.push(id, rect, text);
            }
        }
        // A tip whose subject has left the strip has nothing left to say — and a
        // tip still counting down toward a subject that left has nothing to
        // arrive at. Retiring both here, against the list that was just built, is
        // what keeps the host from owing a frame it could never pay.
        self.tooltip.retain(|id| anchors.find(id).is_some());
        self.tooltip_anchors = anchors;
        // The peek's subject is a tab rather than an anchor, so it is retired
        // against the same predicate that armed it. Sampled into a slice first:
        // the closure cannot read `self` while the host it is retiring is part
        // of `self`.
        let eligible: Vec<bool> = (0..self.tabs.len())
            .map(|index| self.layout_peek_eligible(index))
            .collect();
        self.layout_peek
            .retain(|index| eligible.get(index).copied().unwrap_or(false));
    }

    /// Whether the tip on screen differs from the tip last painted — the strip's
    /// own frame-debt question ([`tab_owes_frame`]), asked about the fade.
    ///
    /// This and not "is it still fading" is what schedules the *landing* frame:
    /// the moment the fade ends there is one more frame owed, carrying the
    /// opacity from wherever the last wake left it up to a solid 1.
    fn tooltip_owes_frame(&self, now: Instant) -> bool {
        self.tooltip_drawn_opacity != self.tooltip_opacity(now)
    }

    /// The opacity the tip should be painted at this instant, or `None` when
    /// there is no tip.
    fn tooltip_opacity(&self, now: Instant) -> Option<f32> {
        self.tooltip
            .active()
            .map(|_| self.tooltip.opacity(now, self.motion))
    }

    /// When this window next has tooltip work: the settle deadline, or the next
    /// frame of a fade that has not landed.
    fn tooltip_deadline(&self, now: Instant) -> Option<Instant> {
        if self.tooltip_owes_frame(now) {
            return Some(now);
        }
        self.tooltip
            .deadline(now, self.motion, STRIP_ANIMATION_FRAME)
    }

    /// Note what the pointer is over and repaint if the answer took a tip down.
    fn note_tooltip(&mut self, anchor: Option<tooltip::TooltipAnchorId>) -> Result<()> {
        if self.tooltip.observe(anchor, Instant::now()) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The anchor under the pointer right now, if any.
    fn tooltip_anchor_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<tooltip::TooltipAnchorId> {
        self.tooltip_anchors
            .at(position.x as f32, position.y as f32)
            .map(|anchor| anchor.id)
    }

    /// Show a settled tip, and keep paying the fade's frames until it lands.
    fn advance_tooltip_if_due(&mut self, now: Instant) -> Result<()> {
        let promoted = self.tooltip.activate_if_due(now);
        if (promoted || self.tooltip_owes_frame(now)) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take the tip down — any press, a lost window, a menu opening (M142, I94).
    fn hide_tooltip(&mut self) -> Result<()> {
        if self.tooltip.hide() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether this tab has a layout worth showing, and whether now is a moment
    /// to show it (L131).
    ///
    /// One predicate, read by both the arming path and the retiring one, because
    /// the two asking different questions is exactly how a popup survives the
    /// death of its own subject.
    fn layout_peek_eligible(&self, tab: usize) -> bool {
        let Some(state) = self.tabs.get(tab) else {
            return false;
        };
        peek_strip::eligible(
            state.seats.pane_count(),
            tab == self.active_tab,
            // A drag owns the pointer outright — the rule the tip, the hyperlink
            // underline and the terminal's own selection already live by.
            self.drag.is_some(),
            // The editor IS the answer, exactly as it is for the tip: a
            // schematic laid over the box you are typing a name into covers the
            // box you are typing it into.
            self.rename
                .as_ref()
                .is_some_and(|editor| editor.tab == state.id),
        )
    }

    /// The tab a peek would belong to, if the pointer is on one that qualifies.
    ///
    /// A tab's own controls count as the tab: `pointerenter`/`pointerleave` do
    /// not fire for a child, so in the mock-up the pointer crossing onto the pin
    /// never leaves the tab, and the schematic stays up.
    fn layout_peek_target_at(&self, position: PhysicalPosition<f64>) -> Option<usize> {
        let tab = match self.chrome_target_at(position)? {
            seats::ChromeTarget::Tab(index)
            | seats::ChromeTarget::TabPin(index)
            | seats::ChromeTarget::TabClose(index) => index,
            _ => return None,
        };
        self.layout_peek_eligible(tab).then_some(tab)
    }

    /// Track the tab under the pointer (L131, L135).
    fn note_layout_peek(&mut self, tab: Option<usize>) -> Result<()> {
        if self.layout_peek.observe(tab, Instant::now()) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Show a settled peek — and, on the same beat, silence the tip.
    ///
    /// §6's whole mechanism is this one line. The peek is due at 350ms and the
    /// tip at 380ms, so promotion always happens first, and promotion is where
    /// the tip stands down. It is *disarmed* rather than merely left undrawn:
    /// a candidate held past its own deadline would report that deadline
    /// forever, and a `WaitUntil` on an instant already in the past is a loop
    /// that never sleeps.
    fn advance_layout_peek_if_due(&mut self, now: Instant) -> Result<()> {
        if !self.layout_peek.activate_if_due(now) {
            return Ok(());
        }
        self.tooltip.hide();
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take the peek down — any press, a lost window, a drag starting (L135).
    fn hide_layout_peek(&mut self) -> Result<()> {
        if self.layout_peek.hide() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether a showing peek has already answered for this anchor.
    ///
    /// The other half of §6, and the half that handles the pointer *moving*
    /// inside a tab whose peek is up: promotion silenced the tip once, and this
    /// is what stops the next mouse-move from arming it again.
    fn layout_peek_suppresses(&self, anchor: tooltip::TooltipAnchorId) -> bool {
        peek_strip::suppresses(self.layout_peek.active(), anchor)
    }

    /// The peek's own layer, or nothing when none is showing.
    ///
    /// Everything is read out of *this* frame — the tree, the names, the focus,
    /// the breath — for the tip's reason: a schematic that remembered the frame
    /// it appeared on would keep showing a pane that has since closed.
    fn layout_peek_layer(&mut self) -> Vec<marks::OverlayLayer> {
        let Some(index) = self.layout_peek.active() else {
            return Vec::new();
        };
        let now = Instant::now();
        let motion = self.motion;
        let scale = self.renderer.metrics().scale_factor as f32;
        let (width, height) = self.renderer.presentation_geometry().swapchain_size;
        let geometry = seats::tab_strip_geometry(
            width as f32,
            scale,
            &self.tab_trailers(now),
            self.active_tab,
            self.tab_scroll,
        );
        let Some(host) = geometry.tabs.get(index).map(|slot| slot.body) else {
            return Vec::new();
        };
        let Some(tab) = self.tabs.get(index) else {
            return Vec::new();
        };
        // The breath belongs to the strip's clock, and it is sampled once here
        // so the mark in the schematic and the mark on the tab are at the same
        // point of the same 1.7s — two clocks would beat against each other.
        let breath = mark_opacity(
            tab.session.status().working,
            false,
            tab.animation_elapsed(now),
            motion,
        );
        let focus = tab.seats.focus();
        let preview_title = tab.preview_image.as_ref().map(PreviewImageState::title);
        // The peek reads the caption through the same door the head does, so a
        // schematic can never name a pane something the pane itself does not.
        let terminal_cwd = tab
            .session
            .working_directory()
            .map(|path| path.to_string_lossy().into_owned());
        let leaves: Vec<peek_strip::PeekLeaf> = tab
            .seats
            .tree()
            .seats_in_order()
            .iter()
            .map(|seat| peek_strip::PeekLeaf {
                kind: seat.kind,
                title: seats::seat_caption(
                    seat.kind,
                    preview_title.as_deref(),
                    terminal_cwd.as_deref(),
                )
                .to_owned(),
                focused: seat.id == focus,
                // Only a terminal has a session that can be working in it.
                mark_opacity: if seat.kind == bt_layout::SeatKind::Terminal {
                    breath
                } else {
                    1.0
                },
            })
            .collect();
        let tree = tab.seats.tree().clone();

        // Only the font knows how wide a name is, so the measuring happens here,
        // beside the renderer, exactly as the tip's does.
        let font_px = peek_strip::LIST_FONT_LOGICAL_PX * scale;
        let widths: Vec<f32> = leaves
            .iter()
            .map(|leaf| self.renderer.measure_chrome_text(&leaf.title, font_px))
            .collect();
        let Some(layout) = peek_strip::layout(
            &tree,
            &leaves,
            &widths,
            host,
            (width as f32, height as f32),
            scale,
        ) else {
            return Vec::new();
        };
        let palette = bt_render::chrome_palette();
        peek_strip::build(&layout, &leaves, &palette, scale)
    }

    /// Fit the open editor's draft into the box the strip has for it.
    ///
    /// The last step of building the strip rather than part of the loop above,
    /// because it is the one piece of tab content that depends on the strip's
    /// own geometry: the draft scrolls to keep its caret in sight, and "in
    /// sight" is a width that only exists once every tab has been given its
    /// share of the run. The measuring is here, beside the renderer, for exactly
    /// the reason the badge's is — only the font knows how wide a word is.
    fn measure_open_rename(&mut self, tabs: &mut [seats::TabContent], scale: f32, width: f32) {
        let Some(tab_id) = self.rename.as_ref().map(|editor| editor.tab) else {
            return;
        };
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let trailers = tabs.iter().map(|tab| tab.trailer).collect::<Vec<_>>();
        let geometry =
            seats::tab_strip_geometry(width, scale, &trailers, self.active_tab, self.tab_scroll);
        let (Some(geometry_tab), Some(content)) = (geometry.tabs.get(index), tabs.get_mut(index))
        else {
            return;
        };
        let Some(title_box) = seats::tab_title_box(
            geometry_tab,
            content.pane_count,
            content.badge_text_width,
            scale,
        ) else {
            // A squeezed tab draws no title, so there is no box to be the editor
            // and nothing to show. The draft is not lost — it is still in
            // `self.rename`, and widening the window brings it back mid-word.
            content.edit = None;
            return;
        };
        let box_width = title_box[2] - title_box[0];
        let font_px = bt_render::WINDOW_TAB_FONT_LOGICAL_PX * scale;
        let caret_width = (seats::TAB_RENAME_CARET_LOGICAL_PX * scale)
            .round()
            .max(1.0);
        // Disjoint fields, split by hand: the editor owns where its window
        // starts and the renderer owns how wide a string is, and this is the one
        // place the two have to meet.
        let renderer = &mut self.renderer;
        let Some(editor) = self.rename.as_mut() else {
            return;
        };
        let mut measure = |text: &str| {
            if text.is_empty() {
                0.0
            } else {
                renderer.measure_chrome_text(text, font_px)
            }
        };
        editor.clamp_scroll();
        // Walk the window's start forward until the caret is inside the box. The
        // caret's own width is held back, because a caret drawn hard against the
        // right edge is a caret half outside it.
        while editor.first_visible < editor.caret
            && measure(&editor.text[editor.first_visible..editor.caret]) > box_width - caret_width
        {
            editor.first_visible += 1;
            while !editor.text.is_char_boundary(editor.first_visible) {
                editor.first_visible += 1;
            }
        }
        // And give the slack back when the text shrinks under it, so deleting
        // from the end reveals the head again instead of leaving the box parked
        // where the longest draft left it. Pulling back only while the *whole*
        // tail still fits cannot undo the loop above: a tail that fits contains
        // a caret that fits.
        while editor.first_visible > 0 {
            let mut candidate = editor.first_visible - 1;
            while !editor.text.is_char_boundary(candidate) {
                candidate -= 1;
            }
            if measure(&editor.text[candidate..]) > box_width {
                break;
            }
            editor.first_visible = candidate;
        }
        let visible = &editor.text[editor.first_visible..];
        let caret_px = measure(&editor.text[editor.first_visible..editor.caret]);
        let selection_px = if editor.select_all {
            measure(visible).min(box_width)
        } else {
            0.0
        };
        content.edit = Some(seats::TabEdit {
            text: visible.to_owned(),
            placeholder: content
                .edit
                .take()
                .map(|edit| edit.placeholder)
                .unwrap_or_default(),
            caret_px,
            selection_px,
            caret_lit: self.rename_blink.visible(),
        });
    }

    /// Where the profile picker hangs right now, or `None` when it is shut.
    fn profile_menu_layout(&self) -> Option<profiles::ProfileMenuLayout> {
        if !self.profile_menu.is_open() {
            return None;
        }
        let scale = self.renderer.metrics().scale_factor as f32;
        let (width, _) = self.renderer.presentation_geometry().swapchain_size;
        let anchor = seats::tab_strip_geometry(
            width as f32,
            scale,
            &self.tab_trailers(Instant::now()),
            self.active_tab,
            self.tab_scroll,
        )
        .new_tab_menu;
        Some(profiles::layout(
            anchor,
            width as f32,
            scale,
            self.recent.entries(),
        ))
    }

    /// The restore prompt's placement, or `None` when it is not asking.
    ///
    /// Every string it draws has to be measured with the real font before the
    /// box that holds them can be sized, which is why the content is built here,
    /// where the renderer is, and handed to a module that knows only numbers.
    fn restore_layout(&mut self) -> Option<restore::RestoreLayout> {
        if !self.restore_prompt.is_open() || self.pending_restore.is_empty() {
            return None;
        }
        let scale = self.renderer.metrics().scale_factor as f32;
        let (width, height) = self.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let renderer = &mut self.renderer;
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(text, size);
        let rows = self
            .pending_restore
            .iter()
            .map(|tab| {
                let seed = first_term_leaf(&tab.root).map_or(
                    seed::Seed::Files {
                        root: String::new(),
                    },
                    |leaf| seed::Seed::Term {
                        profile_id: leaf.profile_id.clone(),
                        cwd: leaf.cwd.clone(),
                        manual_name: leaf.manual_name.clone(),
                    },
                );
                let mut row =
                    restore::RestoreRow::from_seed(&seed, persisted_pane_count(&tab.root));
                row.label_text_width = measure(&row.label, restore::ROW_FONT_LOGICAL_PX * scale);
                row.cwd_text_width = measure(&row.cwd, restore::ROW_CWD_FONT_LOGICAL_PX * scale);
                row.badge_text_width = row.badge_text().map_or(0.0, |text| {
                    measure(&text, bt_render::WINDOW_TAB_BADGE_FONT_LOGICAL_PX * scale)
                });
                row
            })
            .collect();
        let content = restore::RestoreContent {
            rows,
            sub_lines: restore::wrap(
                restore::SUB_TEXT,
                restore::content_width(width, scale),
                |text| measure(text, restore::SUB_FONT_LOGICAL_PX * scale),
            ),
            decline_text_width: measure(
                restore::DECLINE_TEXT,
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
            restore_text_width: measure(
                restore::RESTORE_TEXT,
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
        };
        Some(restore::layout(&content, width, height, scale))
    }

    /// Answer the prompt and put it away.
    fn answer_restore_prompt(&mut self, answer: restore::RestoreAnswer) -> Result<()> {
        self.restore_prompt.close();
        self.answer_restore(answer == restore::RestoreAnswer::Restore)?;
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The settings dialog's placement in the window as it is now, or `None`
    /// when it is shut — or open but unhostable, which is the same thing to
    /// everyone downstream.
    ///
    /// Unhostable has to read as shut and not as "open but invisible": an
    /// invisible modal still swallows Esc and every click, and that is a window
    /// nobody can get out of.
    fn settings_layout(&self) -> Option<settings::SettingsLayout> {
        if !self.settings.is_open() {
            return None;
        }
        let (width, height) = self.renderer.presentation_geometry().swapchain_size;
        settings::layout_for_menu(
            width as f32,
            height as f32,
            self.renderer.metrics().scale_factor as f32,
            self.settings.menu(),
        )
    }

    /// Rebuild the blended layer over the chrome. Returns whether anything
    /// visible changed.
    ///
    /// One layer, because there is only ever one thing in it: the scrim outranks
    /// every popup, so a modal that is up owns the layer outright, and the
    /// picker is closed the moment the dialog opens.
    fn refresh_overlay(&mut self) -> bool {
        // One clock for the whole build, for the reason `tab_trailers` reads one:
        // the drawing and the tween that fades it must not disagree about what
        // time it is, and two `Instant::now()` calls in one frame can.
        let now = Instant::now();
        // Before anything is built, because every layer below is a function of
        // state and this is the state that the pointer, the tree and the window
        // all move.
        self.sync_drop_preview(now);
        // Lowest of everything the overlay carries: `z-index` 24 and 25 against a
        // menu's 30. It is a drawing on the layout rather than a surface over the
        // window, and a dialog that is somehow up during a drag must cover it.
        let mut layers = self.dock_overlay_layers(now);
        layers.extend(if let Some(layout) = self.settings_layout() {
            settings::build(&layout, self.settings.hover(), self.theme_mode)
        } else if let Some(layout) = self.restore_layout() {
            // Above the strip but under no scrim: the prompt floats over a
            // window that already works, which is the whole reason it is
            // allowed to exist (mock-up 2219-2221).
            restore::build(&layout, self.restore_prompt.hover())
        } else if let Some(layout) = self.profile_menu_layout() {
            profiles::build(
                &layout,
                self.profile_menu.hover(),
                self.recent.entries(),
                SystemTime::now(),
            )
        } else {
            Vec::new()
        });
        // Last, and therefore on top of every one of them: `z-index: 60` against
        // the menu's `30` (mock-up 1207). A tip is the only surface in this
        // window that is never covered, because it is the only one whose whole
        // job is to explain what is under it.
        // The tip's own family, immediately under it. The order between these
        // two is unobservable by construction — §6 keeps them from ever being on
        // screen together — so it is fixed here for the reader's sake rather
        // than for the compositor's, and the mock-up's own `z-index` agrees
        // (`.layout-peek` 35, `.tip` 60).
        layers.extend(self.layout_peek_layer());
        layers.extend(self.tooltip_layer());
        // Above even the tip: `z-index: 100` against its `60` (mock-up 1717).
        // The tip's claim to being uncoverable is that it explains what is under
        // the pointer; during a drag, what is under the pointer *is* the ghost,
        // and in practice the two never meet anyway — a drag empties the tip's
        // anchor list (J117's silence, `rebuild_tooltip_anchors`).
        layers.extend(self.drag_ghost_layer());
        let layers = self.settings_marks.resolve_overlay(layers);
        self.renderer.set_modal_overlay(layers)
    }

    /// **K124/N157 — the pane's stand-in in the strip**, and the slot it takes.
    ///
    /// `showDropPreview` (mock-up 6507-6546), which dresses the stand-in as the
    /// tab the pane *would become* — its own mark, its own short name — rather
    /// than as a blank gap. That is the whole reason the ghost goes transparent
    /// over the strip ([`DropLanding::shows_itself`]): the thing under the
    /// pointer and the thing in the slot would otherwise be two labels saying one
    /// name, and only one of them is saying where.
    ///
    /// It wears no pin, no `×` and no pane badge, and none of that is suppressed
    /// here: a stand-in is never the active tab and never hovered, and the strip
    /// already draws those three only for tabs that are. What it does say is
    /// [`seats::TabContent::landing`] at full strength, and that is a reuse rather
    /// than a coincidence — `.drop-preview` and `@keyframes tab-land`'s `from`
    /// are the same two declarations in the mock-up, an accent wash at 9% behind
    /// an inset accent ring at 45%. The slot the drop will fill and the tab that
    /// has just filled it are the same picture, which is what makes the landing
    /// read as the thing you were dragging coming to rest.
    fn strip_stand_in(&self) -> Option<(usize, seats::TabContent)> {
        let drag = self.drag?;
        let DropLanding::StripExtract { slot } = drag.landing? else {
            return None;
        };
        let DragSource::Pane(seat) = drag.source else {
            return None;
        };
        let kind = self.seats.tree().find_seat(seat)?.kind;
        let cwd = self
            .session
            .working_directory()
            .map(|path| path.to_string_lossy().into_owned());
        let title = self
            .preview_image
            .as_ref()
            .map(|preview| preview.title().to_owned());
        Some((
            slot.min(self.tabs.len()),
            seats::TabContent {
                title: seats::seat_short_caption(kind, title.as_deref(), cwd.as_deref()).to_owned(),
                // A pane torn into its own tab holds exactly one pane, and the
                // badge is for tabs that hold more than one (A2/C27). Zero rather
                // than one only because the count is of a tab that does not exist
                // yet; both answers draw nothing, and zero is the honest one.
                pane_count: 0,
                ..seats::TabContent::default()
            },
        ))
    }

    /// Bring the dock drawing up to date with the pointer, the tree and the
    /// window — M155's plan, its cache, and the fade.
    ///
    /// Called from [`Runtime::refresh_overlay`], which is the one choke point
    /// every repaint already passes through, so a resize, a DPI change or a theme
    /// switch re-plans without any of them having to know that a drag is in
    /// flight.
    fn sync_drop_preview(&mut self, now: Instant) {
        let motion = self.motion;
        let Some(inputs) = self.plan_inputs() else {
            self.retire_drop_preview(now, motion);
            return;
        };
        if self
            .drop_preview
            .as_ref()
            .is_none_or(|shown| shown.inputs != inputs)
        {
            let Some(plan) = self.plan_for(&inputs) else {
                // A landing whose plan cannot be built is not a landing that can
                // be drawn. It is not a refusal either — a refusal is a plan that
                // came out too small, and this is the aim naming a seat the tree
                // no longer has. The honest picture of a question with no answer
                // is no picture.
                self.retire_drop_preview(now, motion);
                return;
            };
            // The fade carries across, so moving between zones does not restart
            // it: the box was already up, and the answer changing is a *snap*
            // (M148), never a second arrival.
            let reveal = self.drop_preview.as_ref().map_or_else(
                || RevealTween::over(DOCK_PREVIEW_FADE),
                |shown| shown.reveal,
            );
            self.drop_preview = Some(DropPreview {
                inputs,
                plan,
                reveal,
            });
        }
        if let Some(shown) = self.drop_preview.as_mut() {
            shown.reveal.retarget(1.0, now, motion);
        }
    }

    /// Take the dock drawing down — `hidePreview()` (mock-up 6355-6367).
    ///
    /// It fades rather than vanishing, and it is kept alive for exactly as long
    /// as that takes: the mock-up removes `.show` and leaves the element in the
    /// document for its 100ms, which is what this state is standing in for. Once
    /// the box is gone the plan goes with it, because a plan nobody is drawing is
    /// an answer to a question nobody is asking.
    fn retire_drop_preview(&mut self, now: Instant, motion: Motion) {
        let Some(shown) = self.drop_preview.as_mut() else {
            return;
        };
        shown.reveal.retarget(0.0, now, motion);
        let (reveal, moving) = shown.reveal.sample(now, motion);
        if !moving && reveal <= 0.0 {
            self.drop_preview = None;
        }
    }

    /// What the plan on screen must be a function of, or `None` when there is no
    /// dock to draw.
    fn plan_inputs(&self) -> Option<PlanInputs> {
        self.plan_inputs_for(self.drag?)
    }

    /// The same question asked of a drag the caller is holding.
    ///
    /// [`Runtime::release_drag`] takes the drag out of the window before it
    /// decides anything — a gesture that has ended must not be in flight while
    /// its own consequences run — so the commit cannot ask `self.drag` what it
    /// was aiming at. It asks the drag it has.
    fn plan_inputs_for(&self, drag: Drag) -> Option<PlanInputs> {
        let landing = drag.landing?;
        // The strip's two landings draw themselves in the strip (K124: "the
        // preview in the strip is the ghost now"), so there is no dock box for
        // them and never was one to fade.
        landing.layout_aim()?;
        let cargo = match drag.source {
            DragSource::Pane(_) => None,
            DragSource::Tab(id) => Some(
                self.tabs
                    .iter()
                    .find(|candidate| candidate.id == id)?
                    .seats
                    .tree()
                    .clone(),
            ),
        };
        Some(PlanInputs {
            landing,
            source: drag.source,
            tree: self.seats.tree().clone(),
            cargo,
            viewport: self.seat_viewport,
            scale_ppm: seats::scale_ppm(self.renderer.metrics().dpi_milli().get()),
        })
    }

    /// Run the plan (M155/M156).
    fn plan_for(&self, inputs: &PlanInputs) -> Option<seats::DropPlan> {
        let aim = inputs.landing.layout_aim()?;
        let cargo = match (&inputs.cargo, inputs.source) {
            // M156① — a tab arrives as its whole layout.
            (Some(tree), _) => seats::DropCargo::Layout(tree),
            // M156② — a pane arrives as the seat it already is, fixed column and
            // all.
            (None, DragSource::Pane(seat)) => seats::DropCargo::Pane(seat),
            (None, DragSource::Tab(_)) => return None,
        };
        let mut plan = self
            .seats
            .plan_drop(&self.seat_metrics(), inputs.viewport, aim, cargo)?;
        // A drop this window's tabs cannot hold is refused here rather than at
        // the release, and that ordering is the whole of M147: refuse at the
        // release and the box stays blue right up until the hand opens on
        // nothing, which is the picture "this app is broken" makes too.
        if !tab_can_host(&plan.tree) {
            plan.refuse();
        }
        Some(plan)
    }

    /// The dock drawing's layers: the destinations, then the box over them.
    ///
    /// Under every menu and every tip, because the mock-up puts them there
    /// (`#dock-shift` 24 and `#dock-preview` 25 against `.combo-menu`'s 30 and
    /// `.tip`'s 60) — this is a drawing *on* the layout, not a surface floating
    /// over the window.
    fn dock_overlay_layers(&self, now: Instant) -> Vec<marks::OverlayLayer> {
        let Some(shown) = self.drop_preview.as_ref() else {
            return Vec::new();
        };
        let reveal = shown.reveal.sample(now, self.motion).0;
        if reveal <= 0.0 {
            return Vec::new();
        }
        let overlay = seats::dock_overlay(
            &shown.plan,
            &self.seat_layout,
            self.layout_host_rect(),
            shown.inputs.landing.aimed_at(),
            // A refused box carries no word: what it says is said by being
            // dashed and empty, and "Swap panes" printed inside an outline that
            // means "this will not happen" is the box arguing with itself.
            if shown.plan.fits() {
                shown.inputs.landing.caption(shown.inputs.source)
            } else {
                ""
            },
            self.renderer.metrics().scale_factor as f32,
        );
        let Some(overlay) = overlay else {
            return Vec::new();
        };
        let mut layers = seats::build_dock_overlay(
            &overlay,
            self.renderer.metrics().scale_factor as f32,
            bt_render::chrome_palette(),
        );
        for layer in &mut layers {
            layer.opacity = reveal;
        }
        layers
    }

    /// The ghost's own layer, or nothing when nothing is in the hand (J114-J116).
    ///
    /// Nothing is built for a drag whose landing is already showing itself
    /// ([`DropLanding::shows_itself`]) — not built and then hidden, because an
    /// invisible layer still costs a text shaping pass and a raster lookup every
    /// frame the pointer moves, and "not drawn" is the same picture either way.
    fn drag_ghost_layer(&mut self) -> Vec<marks::OverlayLayer> {
        let Some(drag) = self.drag.filter(Drag::ghost_is_shown) else {
            return Vec::new();
        };
        let palette = bt_render::chrome_palette();
        let Some((mark, mark_logical, mark_color, text)) = self.drag_label(drag, palette) else {
            return Vec::new();
        };
        let scale = self.renderer.metrics().scale_factor as f32;
        // Only the font knows how wide a line is, so the measuring happens here,
        // beside the renderer, exactly as the tip's and the badge's do.
        let width = self
            .renderer
            .measure_chrome_text(&text, bt_render::DRAG_GHOST_FONT_LOGICAL_PX * scale);
        let layout = seats::drag_ghost_layout(
            [drag.pointer.x as f32, drag.pointer.y as f32],
            mark_logical,
            width,
            scale,
        );
        vec![seats::build_drag_ghost(
            &layout, mark, mark_color, &text, scale, palette,
        )]
    }

    /// What the ghost says — `dragLabel(d)`, mock-up 6734-6751.
    ///
    /// "the mark, then the title", and the title is the **short** one: a pane's
    /// head answers "where is this" with the whole path and a label riding the
    /// pointer answers "which one is this" with the last segment alone (C28, and
    /// `seat_short_caption`'s own note). A tab already has exactly one name and
    /// takes it unchanged — `focusedLeaf(tabById(d.wsId))` in the mock-up is how
    /// a tab finds a name at all, and here the tab has been carrying its own
    /// since T1.
    fn drag_label(
        &self,
        drag: Drag,
        palette: bt_render::ChromePalette,
    ) -> Option<(marks::ChromeMark, f32, [u8; 3], String)> {
        match drag.source {
            DragSource::Tab(id) => {
                let tab = self.tabs.iter().find(|candidate| candidate.id == id)?;
                Some((
                    marks::ChromeMark::ProfilePowerShell,
                    bt_render::WINDOW_TAB_MARK_LOGICAL_PX,
                    palette.accent,
                    tab.display_title(),
                ))
            }
            DragSource::Pane(seat) => {
                let kind = self.seats.tree().find_seat(seat)?.kind;
                let (mark, size, colour) = seats::pane_mark(kind, palette);
                let cwd = self
                    .session
                    .working_directory()
                    .map(|path| path.to_string_lossy().into_owned());
                let title = self
                    .preview_image
                    .as_ref()
                    .map(|preview| preview.title().to_owned());
                Some((
                    mark,
                    size,
                    colour,
                    seats::seat_short_caption(kind, title.as_deref(), cwd.as_deref()).to_owned(),
                ))
            }
        }
    }

    /// The tip's own layer, or nothing when none is showing.
    ///
    /// The text is read out of this frame's anchors rather than remembered from
    /// the frame the tip appeared on, so a tab renamed under an open tip says its
    /// new name on the next frame — the mock-up rewrites `el.title` on every
    /// paint for the same reason (line 4331).
    fn tooltip_layer(&mut self) -> Vec<marks::OverlayLayer> {
        // Recorded at the end and only on the paths that actually paint, so the
        // frame-debt comparison is against what is *on screen*. Recording the
        // intent instead would let a tip that could not be laid out report itself
        // as drawn, and the debt would be settled by a frame nobody ever saw.
        self.tooltip_drawn_opacity = None;
        let now = Instant::now();
        let Some(opacity) = self.tooltip_opacity(now) else {
            return Vec::new();
        };
        let Some(anchor) = self
            .tooltip
            .active()
            .and_then(|id| self.tooltip_anchors.find(id))
        else {
            return Vec::new();
        };
        let (text, host) = (anchor.text.clone(), anchor.rect);
        let scale = self.renderer.metrics().scale_factor as f32;
        let font_px = tooltip::TIP_FONT_LOGICAL_PX * scale;
        // Only the font knows how wide a line is, so the measuring happens here,
        // beside the renderer, exactly as the badge's and the editor's do.
        let widths: Vec<f32> = text
            .split('\n')
            .map(|line| self.renderer.measure_chrome_text(line, font_px))
            .collect();
        let (width, height) = self.renderer.presentation_geometry().swapchain_size;
        let Some(layout) =
            tooltip::layout(&text, host, &widths, (width as f32, height as f32), scale)
        else {
            return Vec::new();
        };
        let palette = bt_render::chrome_palette();
        self.tooltip_drawn_opacity = Some(opacity);
        tooltip::build(&layout, &palette, scale, opacity)
    }

    /// The `˅`'s verb: show the profile list, or put away the one on screen.
    fn toggle_profile_menu(&mut self) -> Result<()> {
        self.profile_menu.toggle();
        self.start_chevron_turn();
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Put the picker away and repaint if it was up. Every press that is not the
    /// chevron's and not the menu's own goes through here first, exactly as the
    /// mock-up's document-level `click` handler does.
    fn close_profile_menu(&mut self) -> Result<bool> {
        if !self.profile_menu.close() {
            return Ok(false);
        }
        self.start_chevron_turn();
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Aim the arrow at wherever the list now is.
    ///
    /// Called from both verbs and reading the menu rather than being told which
    /// way to go, so the two can never disagree: whatever put the list up or
    /// down, the arrow's target is one lookup away from the truth. The
    /// *repaint* is the caller's, which is why this only sets the target — a
    /// turn that has begun is finished by `advance_strip_animation` off the
    /// deadline `strip_animation_deadline` asks for.
    fn start_chevron_turn(&mut self) {
        self.chevron_turn
            .retarget(self.profile_menu.is_open(), Instant::now(), self.motion);
    }

    /// The gear's verb: open the dialog, or shut the one that is open.
    ///
    /// Opening drops the chrome's hover state, because the scrim now stands
    /// between the pointer and the gear it is over — the highlight would be a
    /// button claiming to be reachable through a modal.
    fn toggle_settings_panel(&mut self) -> Result<()> {
        self.settings.toggle();
        if self.settings.is_open() {
            self.seat_pointer.hover = None;
            self.apply_pointer_cursor();
        } else if let Some(position) = self.pointer_position {
            self.update_chrome_hover(position)?;
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Route a press that landed on the modal overlay. Every press is consumed:
    /// that is what the scrim is for.
    fn settings_mouse_input(
        &mut self,
        layout: &settings::SettingsLayout,
        state: ElementState,
        button: MouseButton,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return Ok(());
        }
        match settings::hit(layout, position.x, position.y) {
            settings::SettingsTarget::Scrim => self.settings.close(),
            settings::SettingsTarget::Close => self.settings.close(),
            settings::SettingsTarget::ThemeCombo => {
                self.settings.toggle_menu(settings::SettingsMenu::Theme);
            }
            target @ settings::SettingsTarget::ThemeOption(_) => {
                self.settings.set_menu_open(false);
                if let Some(mode) = settings::theme_requested(target) {
                    self.apply_theme_mode(mode)?;
                }
            }
            settings::SettingsTarget::CursorCombo => {
                self.settings.toggle_menu(settings::SettingsMenu::Cursor);
            }
            target @ settings::SettingsTarget::CursorOption(_) => {
                self.settings.set_menu_open(false);
                if let Some(style) = settings::cursor_style_requested(target) {
                    self.apply_cursor_style(style)?;
                }
            }
            // A press on the dialog's own body, or inside the open menu but on
            // none of its items, lands nowhere. It notably does *not* close: the
            // mock-up closes on the scrim and on the `×`, and nothing else.
            settings::SettingsTarget::Panel => {}
            settings::SettingsTarget::ThemeMenu => {}
            settings::SettingsTarget::CursorMenu => {}
        }
        if let Some(position) = self.pointer_position {
            let hover = self
                .settings_layout()
                .map(|layout| settings::hit(&layout, position.x, position.y));
            self.settings.set_hover(hover);
            if !self.settings.is_open() {
                self.update_chrome_hover(position)?;
            }
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Ask the OS for the work area of the display this window is on.
    ///
    /// tiny-window §4.4: a query that fails leaves the last successful answer in
    /// place, because the work area rarely changes between two queries and
    /// reusing the old number is more honest than inventing one. Having never
    /// succeeded is a different state with a different answer — no minimum at
    /// all, rather than a guess that could lock the user's window.
    fn refresh_work_area(&mut self) {
        let Ok(hwnd) = window_hwnd(&self.window) else {
            return;
        };
        let Ok(rect) = bt_platform::get_work_area(hwnd) else {
            return;
        };
        let scale = self.renderer.metrics().scale_factor.max(f64::MIN_POSITIVE);
        let width = ((rect.right - rect.left).max(0) as f64 / scale).round() as i64;
        let height = ((rect.bottom - rect.top).max(0) as f64 / scale).round() as i64;
        self.work_area = WorkAreaHint::Known(bt_layout::LogicalSize::px(width, height));
    }

    /// Hand the OS the largest minimum any tab tree needs (§2.6.5, L12).
    ///
    /// A native window belongs to every tab, not only the active one. Keeping one aggregate also
    /// means activation cannot alternate the OS constraint between two trees.
    ///
    /// The constraint goes to the frame rather than to `Window::set_min_inner_size`: winit 0.30
    /// implements that setter by re-requesting the current inner size, and re-requesting runs
    /// `AdjustWindowRectExForDpi`, which adds a native frame margin this self-drawn window does
    /// not wear. Every call grew the window by that margin. `CustomWindowFrame` states the same
    /// minimum through `WM_GETMINMAXINFO` instead, which asks for no resize at all.
    fn apply_window_min_inner_size(&mut self) -> Result<()> {
        let metrics = self.seat_metrics();
        let minimum = aggregate_window_minimum(self.tabs.iter().map(|tab| {
            tab.seats
                .min_inner_size(&metrics, self.work_area)
                .map(|size| (size.width.floor_px().max(1), size.height.floor_px().max(1)))
        }));
        if !window_minimum_changed(&mut self.window_min_inner_size, minimum) {
            return Ok(());
        }
        self.custom_window_frame
            .set_min_client_size(
                minimum.map(|(width, height)| (width.max(0) as u32, height.max(0) as u32)),
            )
            .map_err(|error| anyhow!(error))
            .context("apply the window's minimum client size")
    }

    /// The durable form of everything this window would want back after a
    /// restart. Layout *intent* only (L11): no rectangle, no cols/rows, no DPI
    /// of a seat — those are all recomputed by the next `solve`.
    fn session_snapshot(&self) -> SessionV1 {
        let mut session = self.session_store.loaded().clone();
        let scale = self.renderer.metrics().scale_factor.max(f64::MIN_POSITIVE);
        let maximized = self.window.is_maximized();
        // The window's *outer* rect, which the self-drawn frame has made the same
        // rectangle as its client area — the one thing `startup_window_rect` can
        // hand back to Win32 without anything in between adjusting it.
        //
        // A maximized window is skipped rather than recorded: its rectangle is the
        // monitor's, not the user's, and writing it would leave the size the user
        // actually chose nowhere to be found — the next unmaximize, and the next
        // start, would both adopt a screen-sized "normal" window. The last
        // rectangle written while normal stands until the window is normal again.
        let bounds = Some(())
            .filter(|()| !maximized)
            .and_then(|()| window_hwnd(&self.window).ok())
            .and_then(|hwnd| bt_platform::get_window_rect(hwnd).ok())
            .map(|rect| persisted_window_bounds(rect, scale))
            .unwrap_or(session.window.bounds);
        session.schema_version = SESSION_SCHEMA_VERSION;
        session.theme = session_theme_mode(self.theme_mode);
        session.cursor_style = session_cursor_style(current_cursor_style());
        session.window = WindowStateV1 {
            bounds,
            dpi: self.renderer.metrics().dpi_milli().get(),
            maximized,
            monitor_id: session.window.monitor_id.clone(),
        };
        session.tabs = self
            .tabs
            .iter()
            .map(|tab| TabV1 {
                root: tab.seats.to_persisted(&tab.term_leaf()),
                pinned: tab.pinned,
                // Positional rather than a stable id: the in-order index is a function of the
                // same tree shape the file carries, so it cannot point outside that tree.
                focused_leaf: format!("leaf-{}", focus_leaf_index(&tab.seats)),
            })
            .collect();
        session.active_tab = self.active_tab as u32;
        // A question that was never answered is not a "no". Tabs still waiting on
        // the restore prompt go back to the file exactly as they came out of it,
        // so closing the window mid-question asks again next time rather than
        // deciding on the user's behalf (§7.1.4: "未答复计划并回 lastSession,
        // 不得丢失"). They are appended unpinned, which is what they were.
        session
            .tabs
            .extend(self.pending_restore.iter().map(|tab| TabV1 {
                pinned: false,
                ..tab.clone()
            }));
        // The vault is app state while the window is up and file state the moment
        // it is not; this is the one place the two meet.
        session.recent = self.recent.to_persisted();
        session
    }

    /// Record a meaningful change and start the debounce window (§5.1).
    fn mark_session_dirty(&mut self, now: Instant) {
        let snapshot = self.session_snapshot();
        self.session_store.record(snapshot, now);
    }

    /// Commit every theme-dependent surface at one event-loop safe point. Until the resulting frame
    /// presents, DWM retains the previous complete back buffer; the renderer never submits a frame
    /// with only one side of this transaction applied.
    fn apply_theme_mode(&mut self, mode: ThemeModeV1) -> Result<bool> {
        let mode_changed = self.theme_mode != mode;
        self.theme_mode = mode;
        let theme_changed = self.apply_theme(resolve_theme_mode(mode, self.window.theme()))?;
        if mode_changed {
            self.mark_session_dirty(Instant::now());
            if !theme_changed && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
        }
        Ok(mode_changed || theme_changed)
    }

    fn os_theme_changed(&mut self, os_theme: OsTheme) -> Result<bool> {
        let Some(theme) = resolved_theme_change(self.theme_mode, os_theme) else {
            return Ok(false);
        };
        self.apply_theme(theme)
    }

    fn apply_theme(&mut self, theme: Theme) -> Result<bool> {
        match set_theme(theme) {
            ThemeChange::LockedByEnvironment => {
                eprintln!(
                    "BT_THEME switch_ignored={theme:?} reason=BT_BG runtime_theme_locked=true"
                );
                Ok(false)
            }
            ThemeChange::Unchanged => Ok(false),
            ThemeChange::Changed => {
                install_theme_class_background(&self.window)?;
                self.sync_math_layout_key();
                self.refresh_chrome();
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })?;
                Ok(true)
            }
        }
    }

    fn apply_cursor_style(&mut self, style: CursorStyle) -> Result<bool> {
        if !set_cursor_style(style) {
            return Ok(false);
        }
        self.mark_session_dirty(Instant::now());
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// The dev-only preview toggle, and everything one costs: the tree changes,
    /// so the window minimum, the terminal's columns, the ConPTY coalescer and
    /// the session file all follow it, in that order.
    fn toggle_preview_seat(&mut self) -> Result<()> {
        let metrics = self.seat_metrics();
        let was_open = self.seats.preview().is_some();
        if !self.seats.toggle_preview(&metrics) {
            return Ok(());
        }
        if was_open {
            self.preview_image = None;
            self.renderer.set_preview_image(None);
        }
        self.seat_pointer = seats::ChromePointer::default();
        self.divider_drag = None;
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()
    }

    /// `closePane` (mock-up 3558-3578, I102/I103/I105).
    ///
    /// One verb for every kind of leaf: the leaf leaves the tree, its sibling is
    /// promoted, and the run it left is re-balanced — all of which `close_seat`
    /// already does, because G79/G80 rule that leaving is balanced exactly as
    /// joining is.
    ///
    /// The branch that is new here is the last one. `detachLeaf` returns null
    /// when the tree holds a single pane, and the mock-up's answer to that is not
    /// "refuse" but `closeTab(w.id)` — **an empty tab is not a state that
    /// exists** (T226/§2.1), so closing the last pane *is* closing the tab. That
    /// falls through to `close_tab`, which keeps its own rule about the last tab
    /// in the strip: the window does not empty either.
    fn close_pane(&mut self, seat: bt_layout::SeatId) -> Result<()> {
        if self.seats.pane_count() <= 1 {
            return self.close_tab(self.active_tab);
        }
        let kind = self
            .seat_layout
            .get(seat)
            .map(|placement| placement.kind)
            .unwrap_or(bt_layout::SeatKind::Terminal);
        let metrics = self.seat_metrics();
        if !self.seats.close_seat(&metrics, seat) {
            return Ok(());
        }
        // A preview seat holds an image the way a terminal holds a session, and
        // the pane going away is the one taking it.
        if kind == bt_layout::SeatKind::Preview {
            self.preview_image = None;
            self.renderer.set_preview_image(None);
        }
        // A terminal seat holds a shell, and the pane going away takes that too.
        // Closing the pane and leaving the ConPTY alive would leak a process
        // with nothing to draw it and nothing to read it — the pipe fills, and
        // the child blocks forever on a write no one will ever drain.
        if kind == bt_layout::SeatKind::Terminal
            && let Some(mut leaf) = self.sessions.remove(&seat)
        {
            if let Some(pty) = leaf.pty.as_mut() {
                pty.shutdown().context("shut down closed pane's shell")?;
            }
            // Keyboard focus cannot stay on a seat that no longer exists. The
            // layout has already promoted a sibling; follow it to whichever
            // terminal is still standing.
            if self.focused_leaf == seat
                && let Some(next) = self.sessions.keys().next().copied()
            {
                self.focused_leaf = next;
            }
        }
        // The pointer's whole picture is stale: the rectangle it was over has
        // been re-solved out from under it, and a `pane_hover` naming a seat
        // that no longer exists would keep a `×` lit on a pane that is gone.
        self.seat_pointer = seats::ChromePointer::default();
        self.divider_drag = None;
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()?;
        if let Some(position) = self.pointer_position {
            self.update_chrome_hover(position)?;
        }
        self.mark_session_dirty(Instant::now());
        Ok(())
    }

    /// Split the focused terminal pane, seating a second shell beside it.
    ///
    /// The creation entry U12 was missing. Two things happen together and must:
    /// the tree gains a Terminal leaf, and that leaf gains a shell. A seat with
    /// no session is a black rectangle nothing will ever draw into, and a
    /// session with no seat is a process nobody can see or reach — so the solver
    /// is asked first, and if it refuses the split for want of room, nothing at
    /// all is spawned.
    ///
    /// I88's rule for a new tab, read for a new pane: the arriving shell opens
    /// where the pane it was split from is standing, by that pane's own OSC 7
    /// report. A pane whose shell has never named a directory hands over
    /// nothing, and the new shell starts where it always did.
    fn split_focused_terminal(&mut self, dir: Axis) -> Result<()> {
        let metrics = self.seat_metrics();
        let source = self.focused_leaf;
        // Ask the solver first. `split_terminal` leaves the tree untouched when
        // it refuses, so there is nothing to undo on this path.
        let Some(arriving) = self.seats.split_terminal(&metrics, source, dir, false) else {
            return Ok(());
        };
        // Re-solve before spawning: the new pane's shell has to be told how many
        // columns it has, and that answer comes from the solve the split just
        // changed — never invented here (red line L10).
        self.commit_seat_geometry()?;
        let scale = self.renderer.metrics().scale_factor as f32;
        let Some(body) = seats::pane_body_viewport(&self.seats, &self.seat_layout, arriving, scale)
        else {
            // The solver placed no rectangle for the seat it just minted, which
            // would mean the tree and its solve disagree. Undo the split rather
            // than leave a seat nothing can draw.
            self.seats.close_seat(&metrics, arriving);
            self.commit_seat_geometry()?;
            return Ok(());
        };
        let inherited = self
            .sessions
            .get(&source)
            .and_then(|leaf| leaf.session.working_directory().map(Path::to_path_buf));
        let proxy = self.event_proxy.clone();
        let wake: OutputWake = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::PtyOutput);
        });
        let leaf = create_leaf_session(&self.renderer, body, wake, None, inherited)?;
        self.sessions.insert(arriving, leaf);
        // Focus follows the split, keyboard and layout together: you split in
        // order to work in the new pane.
        self.focused_leaf = arriving;
        self.seats.set_focus(arriving);
        self.seat_pointer = seats::ChromePointer::default();
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()?;
        self.refresh_chrome();
        self.mark_session_dirty(Instant::now());
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    /// D40: pressing anywhere in a pane moves the layout focus there.
    ///
    /// Anywhere at all — the head, the terminal's own body, a preview's body —
    /// which is what `document.querySelectorAll(".pane")` listening for `click`
    /// means at mock-up 5823-5834. It runs *above* the chrome router and never
    /// consumes the press: focus is what the press means in addition to whatever
    /// else it meant, and a press in a terminal still has a selection to start.
    ///
    /// This is the second half that comment promised. Layout focus is still the
    /// solver's input to W2 and to the collapse order, and it still moves for a
    /// press on any leaf; but a tab now holds a shell per terminal leaf, so a
    /// press in a terminal pane also moves the *keyboard* there — the caret, the
    /// selection and the IME follow the hand, because there is now somewhere for
    /// them to follow it to.
    ///
    /// Keyboard focus moves only for a terminal pane. Pressing a files column or
    /// a preview must not take the keyboard away from the shell you were typing
    /// in and hand it to a pane that cannot accept a keystroke — so those move
    /// layout focus alone, exactly as before.
    ///
    /// It runs above the router and consumes nothing, which is what makes the
    /// ordering work: focus lands first, and the same press then starts its
    /// selection in the pane it just focused, through the ordinary path.
    fn focus_pane_at(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        let Some(seat) = seats::pane_at(&self.seat_layout, position.x, position.y) else {
            return Ok(());
        };
        // Keyboard first, and independently of whether layout focus moved: the
        // two can already disagree when a pane was focused by other means.
        if self.sessions.contains_key(&seat) && self.focused_leaf != seat {
            self.focused_leaf = seat;
            // The frame slot holds the pane that *was* focused. Leaving it would
            // let the next present assert a stale grid against the new pane.
            self.last_presented_frame = None;
        }
        if !self.seats.set_focus(seat) {
            return Ok(());
        }
        // Focus is W2's input, so the window's own minimum can move with it —
        // the focus seat is the last one the concession ladder folds.
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()?;
        self.mark_session_dirty(Instant::now());
        Ok(())
    }

    /// Open the ruled preview seat if necessary, otherwise reuse its geometry, then ask the shared
    /// worker/cache pipeline for this image. Keyboard focus deliberately remains on the terminal.
    fn open_preview_image(&mut self, path: PathBuf) -> Result<()> {
        self.preview_image = Some(PreviewImageState::new(path));
        self.renderer.set_preview_image(None);
        if self.seats.preview().is_none() {
            return self.toggle_preview_seat();
        }
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    fn defer_preview_resample(&mut self, observed_at: Instant) {
        if let Some(preview) = self.preview_image.as_mut() {
            preview.defer_resize_scale(observed_at);
        }
    }

    fn finish_preview_resize_if_quiet(&mut self, now: Instant) -> Result<()> {
        let due = self
            .preview_image
            .as_mut()
            .is_some_and(|preview| preview.finish_resize_scale_if_quiet(now));
        if due {
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Refit the current image to the solver's latest preview body. Decodes stay on the decoration
    /// worker and Lanczos3 runs on its independent scale lane; this method only routes shared data.
    fn refresh_preview_for_layout(&mut self) {
        let Some(preview_seat) = self.seats.preview() else {
            self.renderer.set_preview_image(None);
            return;
        };
        let scale = self.renderer.metrics().scale_factor as f32;
        let Some(body) =
            seats::preview_body_viewport(&self.seats, &self.seat_layout, preview_seat, scale)
        else {
            self.renderer.set_preview_image(None);
            return;
        };
        let Some(path) = self
            .preview_image
            .as_ref()
            .map(|preview| preview.path.clone())
        else {
            self.renderer.set_preview_image(None);
            return;
        };
        let cache_key = normalized_local_image_path_key(&path);
        let decoded = match self.peek_cache.get(&cache_key) {
            Some(PeekCacheEntry::Ready {
                key,
                rgba,
                width_px,
                height_px,
            }) => Some((key.clone(), Arc::clone(rgba), *width_px, *height_px)),
            Some(PeekCacheEntry::Pending) => {
                self.renderer.set_preview_image(None);
                return;
            }
            Some(PeekCacheEntry::Failed) => {
                if let Some(preview) = self.preview_image.as_mut() {
                    preview.failure.get_or_insert_with(|| {
                        "Preview failed: image could not be loaded".to_owned()
                    });
                }
                self.renderer.set_preview_image(None);
                return;
            }
            None => None,
        };
        if let (Some(preview), Some((_, _, native_width, native_height))) =
            (self.preview_image.as_mut(), decoded.as_ref())
        {
            preview.native = Some((*native_width, *native_height));
        }
        let Some((content_key, rgba, native_width, native_height)) = decoded else {
            self.renderer.set_preview_image(None);
            if !self.math_worker_running {
                if let Some(preview) = self.preview_image.as_mut() {
                    preview.failure =
                        Some("Preview failed: image worker is unavailable".to_owned());
                }
                return;
            }
            if self
                .math_worker
                .tasks
                .send(MathWorkerRequest::PeekImage {
                    tab_id: self.id,
                    path,
                })
                .is_ok()
            {
                self.peek_cache.insert(cache_key, PeekCacheEntry::Pending);
            } else if let Some(preview) = self.preview_image.as_mut() {
                preview.failure = Some("Preview failed: image worker is unavailable".to_owned());
            }
            return;
        };
        // Breathing room: fit against an inset body so the picture never touches the seat's
        // edges. A body too small to afford the margin gets the full rectangle instead — the
        // margin exists to serve the picture, not to starve it.
        let inset = (PREVIEW_BODY_INSET_LOGICAL_PX * scale).round().max(0.0) as u32;
        let (fit_width, fit_height) = if body.width > inset * 4 && body.height > inset * 4 {
            (body.width - 2 * inset, body.height - 2 * inset)
        } else {
            (body.width, body.height)
        };
        let Some((display_width, display_height)) =
            preview_image_extent(fit_width, fit_height, native_width, native_height)
        else {
            if let Some(preview) = self.preview_image.as_mut() {
                preview.failure = Some("Preview failed: preview seat is too small".to_owned());
            }
            self.renderer.set_preview_image(None);
            return;
        };
        let target = (content_key.clone(), display_width, display_height);
        let exact_raster = self
            .preview_image
            .as_ref()
            .and_then(|preview| preview.raster.as_ref())
            .is_some_and(|raster| raster.matches(&target));
        if let Some(raster) = self
            .preview_image
            .as_ref()
            .and_then(|preview| preview.raster.as_ref())
        {
            // During a drag, keep the last texture on screen and let the sampler stretch it to the
            // new fitted extent. It may be briefly soft, but the preview never vanishes; quiet-time
            // delivery below replaces it with a one-to-one display raster.
            self.renderer.set_preview_image(Some(PreviewImage {
                seat: body,
                key: raster.key.clone(),
                rgba: Arc::clone(&raster.rgba),
                width_px: raster.width_px,
                height_px: raster.height_px,
                display_width_px: display_width,
                display_height_px: display_height,
            }));
        } else {
            self.renderer.set_preview_image(None);
        }
        if exact_raster {
            return;
        }
        if self.preview_image.as_ref().is_some_and(|preview| {
            preview.pending.as_ref() == Some(&target) || preview.resize_scale_deadline.is_some()
        }) || !self.math_worker_running
        {
            return;
        }
        let task = peek_scale_task(&target, rgba, native_width, native_height);
        if self
            .math_worker
            .scale_tasks
            .send(ScaleWorkerRequest::Preview {
                tab_id: self.id,
                task,
            })
            .is_ok()
        {
            if let Some(preview) = self.preview_image.as_mut() {
                preview.pending = Some(target);
                preview.failure = None;
            }
        } else if let Some(preview) = self.preview_image.as_mut() {
            preview.failure = Some("Preview failed: image worker is unavailable".to_owned());
        }
    }

    /// Re-solve after a tree edit and carry the consequences to the terminal.
    ///
    /// Deliberately routed through the same coalescer a window resize uses: a
    /// divider drag and an OS resize are the same event as far as ConPTY is
    /// concerned, and §4.2 says the solver does not participate in that
    /// debounce — it answers every frame, and someone else decides when the
    /// child hears about it.
    fn commit_seat_geometry(&mut self) -> Result<()> {
        let trace_started = self.trace_perf.then(Instant::now);
        let render_physical = presentation_physical_size(self.renderer.presentation_geometry());
        if render_physical.width == 0 || render_physical.height == 0 {
            return Ok(());
        }
        // A seat rectangle changing is a resize as far as a transient flyout is
        // concerned: its anchor was a physical point on the old pane and its
        // raster was sized to it. tiny-window §3.5 generalises the existing
        // dissolve rule to exactly this case, so the same two lines `resize`
        // already runs run here.
        self.peek_hover.clear();
        self.renderer.set_peek_overlay(None);
        let now = Instant::now();
        self.defer_preview_resample(now);
        let next_grid = self.resolve_seat_layout(render_physical);
        // The panes without the keyboard, before the focused one: they take the
        // solver's answer unconditionally, so doing them first keeps the focused
        // leaf's typed-input gate the last word rather than a thing another
        // pane's resize could race.
        self.resize_unfocused_leaves()?;
        let solved_at = trace_started.map(|_| Instant::now());
        self.schedule_grid_change(
            next_grid,
            terminal_pty_physical(&self.renderer, render_physical),
            now,
            "resize terminal actor for a seat layout change",
        )?;
        let resized_at = trace_started.map(|_| Instant::now());
        self.sync_math_layout_key();
        // The grid actually in force, which under the typed-input gate is still the old one. The
        // present gate admits the grid the frame will really carry, never the one merely solved.
        self.pending_resize_present = Some(self.grid);
        self.mark_session_dirty(now);
        self.publish_frame(FrameTrigger {
            occurred_at: now,
            source: FrameSource::Resize,
        })?;
        let published_at = trace_started.map(|_| Instant::now());
        let synchronous_present = self.divider_drag.is_none();
        // Pointer motion must stay ahead of the swapchain. `publish_frame` already requested a
        // redraw and `LatestFrameSlot` keeps the newest geometry, so presenting synchronously here
        // would make every divider event wait on GPU acquire/vsync before Windows can deliver the
        // next event. Non-drag seat edits still present immediately; a live drag is frame-paced by
        // RedrawRequested and may coalesce only superseded intermediate positions.
        if synchronous_present {
            self.redraw()?;
        }
        if let (Some(started), Some(solved), Some(resized), Some(published)) =
            (trace_started, solved_at, resized_at, published_at)
        {
            eprintln!(
                "BT_PERF_TRACE resize_frame solve_us={} actor_us={} publish_us={} redraw_us={} total_us={} queued={} columns={} rows={}",
                solved.saturating_duration_since(started).as_micros(),
                resized.saturating_duration_since(solved).as_micros(),
                published.saturating_duration_since(resized).as_micros(),
                Instant::now()
                    .saturating_duration_since(published)
                    .as_micros(),
                started.elapsed().as_micros(),
                u8::from(!synchronous_present),
                next_grid.columns,
                next_grid.rows,
            );
        }
        Ok(())
    }

    /// The pointer, expressed in the terminal seat's own coordinates, or `None`
    /// when it is not over the terminal seat at all.
    ///
    /// Every existing hit test — the grid, math blocks, hyperlinks, the peek
    /// flyout's anchor — reads through here, so all of them keep working with
    /// exactly one correction applied in exactly one place.
    fn terminal_pointer(&self) -> Option<PhysicalPosition<f64>> {
        let position = self.pointer_position?;
        let seat = self.renderer.seat_viewport();
        if !seats::terminal_contains(
            &self.seat_layout,
            self.seats.terminal(),
            position.x,
            position.y,
        ) {
            return None;
        }
        Some(PhysicalPosition::new(
            position.x - f64::from(seat.x),
            position.y - f64::from(seat.y),
        ))
    }

    fn publish_frame(&mut self, trigger: FrameTrigger) -> Result<()> {
        let skip_unchanged = matches!(trigger.source, FrameSource::PtyOutput);
        self.publish_frame_inner(trigger, skip_unchanged)
            .map(|_| ())
    }

    fn publish_frame_inner(&mut self, trigger: FrameTrigger, skip_unchanged: bool) -> Result<bool> {
        // Real-machine decoration-state trace (`BT_DECOR_TRACE=<path>`). Runs on every frame trigger
        // — including held/skipped frames — so a persistent stuck-source block is captured even when
        // the presented frame does not change. Zero cost when the variable is unset.
        self.session.trace_decorations();
        if matches!(trigger.source, FrameSource::Keyboard) {
            self.session.release_presentation_hold_for_user_input();
        }
        let active = self.active_tab;
        let tasks = self.math_worker.tasks.clone();
        let scale_tasks = self.math_worker.scale_tasks.clone();
        dispatch_pending_math_tasks(
            self.tabs[active].id,
            &mut self.tabs[active].session,
            &tasks,
            &scale_tasks,
            &mut self.math_worker_running,
            &mut self.math_worker_notice_pending,
        );
        let mut terminal_frame = {
            // Bound once, to the focused leaf: `session` and `projection` are
            // two fields of one shell, and reaching each through its own deref
            // would be two borrows of the tab rather than one borrow of the
            // leaf.
            let leaf = self.tabs[active].focused_mut();
            leaf.session.refresh_projection(&mut leaf.projection);
            leaf.session
                .viewport_frame(&mut leaf.projection)
                .context("project terminal grid into viewport frame")?
        };
        // State-driven frame hold. Review displacement holds a vanished scroll anchor during a
        // resize reprint. Independently, an unmatched off-band stale-pending DPI record holds the
        // previous complete formula frame while a proven primary reprint is between clear and exact
        // source re-anchor. Both release through projection/session facts (re-anchor, explicit user
        // takeover, or hard lifecycle retirement), never a timer.
        if self.projection.presentation_hold() && self.last_presented_frame.is_some() {
            if self.trace_perf {
                eprintln!(
                    "BT_PERF_TRACE hold=presentation source={:?} review={} exact_source={}",
                    trigger.source,
                    u8::from(self.projection.review_hold()),
                    u8::from(self.projection.exact_source_reprint_hold()),
                );
            }
            return Ok(false);
        }
        if self.session.schedule_visible_artifacts(&terminal_frame) != 0 {
            dispatch_pending_math_tasks(
                self.tabs[active].id,
                &mut self.tabs[active].session,
                &tasks,
                &scale_tasks,
                &mut self.math_worker_running,
                &mut self.math_worker_notice_pending,
            );
        }
        if let Some(hyperlink) = self.hyperlink_hover.underline_target()
            && terminal_frame.underline_hyperlink(hyperlink)
            && self.hyperlink_hover.active.is_some()
        {
            terminal_frame.status_text = self
                .hyperlink_hover
                .status_text(terminal_frame.columns.get() as usize);
        }
        // This frame's own references, scanned once for the whole of this frame's life on screen.
        // The hover upgrade below, the Ctrl+click verb and the peek all read this one list, so no
        // two of them can disagree about where a reference is or whether it is one. The session
        // scanned the same frame once more when it painted the resting dots inside `viewport_frame`;
        // collapsing the two would mean hanging the scan on the session as state, which is the very
        // kind of thing this affordance was rebuilt to be rid of.
        self.frame_image_references = FrameImageReferences {
            columns: terminal_frame.columns.get(),
            references: self.session.frame_image_references(&terminal_frame),
        };
        // The verified reference under the pointer turns solid, on the same terms the link does:
        // the underline is the affordance and follows the pointer immediately, while the 300ms
        // settle belongs to what the hover *reveals* — a tooltip there, a thumbnail here.
        let hovered_reference = self.hovered_image_reference();
        if let Some(reference) = hovered_reference.as_ref() {
            terminal_frame.underline_cells(&reference.cells, true);
        }
        self.underlined_image_reference = hovered_reference;
        if let Some(notice) = take_math_worker_notice(&mut self.math_worker_notice_pending) {
            terminal_frame.status_text = Some(notice.to_owned());
        }
        if let Some(notice) = self.shell_fallback_notice.take() {
            terminal_frame.status_text = Some(notice);
        }
        let composed = compose_preedit(&terminal_frame, self.preedit.as_ref())
            .context("reject non-rectangular frame before IME composition")?;
        if skip_unchanged
            && pty_frame_is_unchanged(
                self.pending_frames.pending_frame(),
                self.last_presented_frame.as_ref(),
                &composed.frame,
            )
        {
            if self.trace_perf {
                let digest_started = Instant::now();
                let digest = frame_content_digest(&composed.frame);
                let alternate_screen = frame_is_alternate_screen(&composed.frame);
                let digest_elapsed = digest_started.elapsed();
                eprintln!(
                    "BT_PERF_TRACE skip=unchanged source={:?} content_fnv={:016x} alt={} digest_us={}",
                    trigger.source,
                    digest.content_fnv,
                    u8::from(alternate_screen),
                    digest_elapsed.as_micros(),
                );
            }
            return Ok(false);
        }
        if self.ime_active {
            let area = self.renderer.ime_cursor_area(&composed.frame);
            if let Some(area) = self.ime_cursor_throttle.offer(area, Instant::now()) {
                self.apply_ime_cursor_area(area);
            }
        }
        self.session
            .record_published_frame(&composed.frame, trigger.occurred_at);
        self.flush_resize_trace();
        self.pending_frames
            .publish(composed.frame, trigger)
            .context("reject non-rectangular frame at publish boundary")?;
        self.window.request_redraw();
        Ok(true)
    }

    fn disable_math_worker(&mut self) -> bool {
        disable_math_worker_state(
            &mut self.math_worker_running,
            &mut self.math_worker_notice_pending,
        )
    }

    fn apply_math_results(&mut self) -> Result<()> {
        let mut changed = false;
        loop {
            match self.math_worker.results.try_recv() {
                Ok(completion) => {
                    let target_index = self.tabs.iter().position(|tab| tab.id == completion.tab_id);
                    let target_active = target_index == Some(self.active_tab);
                    changed |= match completion.completion {
                        DecorationWorkerCompletion::Math { task, result } => match *task {
                            SessionMathTask::Frozen(task) => target_index.is_some_and(|index| {
                                let applied = self.tabs[index]
                                    .session
                                    .complete_worker_result(task, result);
                                target_active && applied
                            }),
                            SessionMathTask::Live(task) => target_index.is_some_and(|index| {
                                let applied = self.tabs[index]
                                    .session
                                    .complete_live_worker_result(task, result);
                                target_active && applied
                            }),
                        },
                        DecorationWorkerCompletion::InlineImage { task, result } => {
                            if target_active {
                                self.remember_decode_for_peek(&task, result.as_ref().ok());
                            }
                            target_index.is_some_and(|index| {
                                let applied = self.tabs[index]
                                    .session
                                    .complete_inline_image_result(task, result);
                                target_active && applied
                            })
                        }
                        DecorationWorkerCompletion::ScaleInlineImage { scaled } => target_index
                            .is_some_and(|index| {
                                let applied =
                                    self.tabs[index].session.complete_inline_image_scale(scaled);
                                target_active && applied
                            }),
                        DecorationWorkerCompletion::PeekImage { path, result } => {
                            if target_active {
                                self.complete_peek_image(path, result)?;
                            }
                            // Peek state never enters frames, so no republish is needed.
                            false
                        }
                        DecorationWorkerCompletion::PeekScaledImage { scaled } => {
                            if target_active {
                                self.complete_peek_scale(scaled)?;
                            }
                            false
                        }
                        DecorationWorkerCompletion::PreviewScaledImage { scaled } => {
                            if target_active {
                                self.complete_preview_scale(scaled)?;
                            }
                            false
                        }
                    };
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    changed |= self.disable_math_worker();
                    break;
                }
            }
        }
        let active = self.active_tab;
        let tasks = self.math_worker.tasks.clone();
        let scale_tasks = self.math_worker.scale_tasks.clone();
        dispatch_pending_math_tasks(
            self.tabs[active].id,
            &mut self.tabs[active].session,
            &tasks,
            &scale_tasks,
            &mut self.math_worker_running,
            &mut self.math_worker_notice_pending,
        );
        if changed {
            self.publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    fn advance_live_math_if_due(&mut self, now: Instant) -> Result<()> {
        if self
            .session
            .live_stability_deadline()
            .is_some_and(|deadline| now >= deadline)
        {
            self.session.advance_live_stability(now);
            let active = self.active_tab;
            let tasks = self.math_worker.tasks.clone();
            let scale_tasks = self.math_worker.scale_tasks.clone();
            let disabled = dispatch_pending_math_tasks(
                self.tabs[active].id,
                &mut self.tabs[active].session,
                &tasks,
                &scale_tasks,
                &mut self.math_worker_running,
                &mut self.math_worker_notice_pending,
            );
            if disabled {
                self.publish_frame(FrameTrigger {
                    occurred_at: now,
                    source: FrameSource::Expose,
                })?;
            }
        }
        Ok(())
    }

    fn publish_pty_drain_frame(&mut self, now: Instant, force: bool) -> Result<()> {
        let keyboard_at = self.pending_keyboard_at;
        let published = self.publish_frame_inner(
            FrameTrigger {
                occurred_at: keyboard_at.unwrap_or(now),
                source: if keyboard_at.is_some() {
                    FrameSource::Keyboard
                } else {
                    FrameSource::PtyOutput
                },
            },
            !force,
        )?;
        let sync_open = self.session.synchronized_update_deadline().is_some();
        if published || !sync_open {
            self.pending_keyboard_at = None;
        } else if self.trace_perf {
            eprintln!("BT_PERF_TRACE defer=synchronized-update");
        }
        Ok(())
    }

    fn flush_resize_trace(&mut self) {
        if !self.trace_resize {
            return;
        }
        let transaction = self.session.resize_trace_transaction();
        if transaction != self.resize_trace_logged_transaction {
            self.resize_trace_logged_transaction = transaction;
            self.resize_trace_logged_events = 0;
        }
        let trace = self.session.resize_trace();
        let conpty_source = self
            .pty
            .as_ref()
            .map(|pty| pty.conpty_source().to_string())
            .unwrap_or_else(|| "direct-input".to_string());
        for event in &trace[self.resize_trace_logged_events.min(trace.len())..] {
            eprintln!("BT_RESIZE_TRACE conpty_source={conpty_source:?} {event:?}");
        }
        self.resize_trace_logged_events = trace.len();
    }

    fn apply_ime_cursor_area(&mut self, area: ImeCursorArea) {
        // Renderer pixels, winit PhysicalPosition, and a per-monitor-aware Win32 client area all
        // share the client-origin device-pixel axis. No screen-origin translation belongs here.
        //
        // A seat-origin one does. The caret rectangle is computed by the same
        // frame machinery as every other content pixel, so it is expressed in
        // the terminal seat's own coordinates; winit and IMM32 both want the
        // window's. This is the inverse of `terminal_pointer`'s correction and
        // the only place it runs in this direction (§4.1, one translation).
        let area = window_ime_cursor_area(self.renderer.seat_viewport(), area);
        self.window.set_ime_cursor_area(
            PhysicalPosition::new(area.x, area.y),
            PhysicalSize::new(area.width, area.height),
        );
        if let Err(error) = self.ime_system_caret.update(area.x, area.y) {
            eprintln!("Chinese IME system-caret update ignored: {error}");
        }
    }

    fn flush_ime_cursor_area(&mut self, now: Instant) {
        if let Some(area) = self.ime_cursor_throttle.flush_due(now) {
            self.apply_ime_cursor_area(area);
        }
    }

    fn drain_pty(&mut self) -> Result<()> {
        let mut active_changed = false;
        let mut chrome_changed = false;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let (changed, title_changed) = drain_tab_pty(tab)?;
            if index == self.active_tab {
                active_changed = changed;
            }
            chrome_changed |= title_changed;
        }
        if chrome_changed {
            self.window.set_title(&self.display_title());
            self.refresh_chrome();
            if !active_changed {
                self.present_chrome_change()?;
            }
        }
        if active_changed {
            let now = Instant::now();
            let cursor_revealed = self.reset_cursor_blink(now);
            // The vendor parser withholds bytes inside an open DEC 2026 block, so projecting here
            // cannot expose its intermediate state. It can expose ordinary output before a
            // trailing BSU or a completed update before the next BSU; the unchanged-frame gate in
            // publish_frame cheaply suppresses drains containing only still-buffered sync bytes.
            self.publish_pty_drain_frame(now, cursor_revealed)?;
        }
        Ok(())
    }

    fn finish_synchronized_update_if_due(&mut self, now: Instant) -> Result<()> {
        let mut active_finished = false;
        let mut chrome_changed = false;
        let active = self.active_tab;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            // Per leaf: a synchronized update is a property of one screen, and
            // two shells in one tab time out independently.
            for (_, leaf) in tab.leaves_mut() {
                let due = leaf
                    .session
                    .synchronized_update_deadline()
                    .is_some_and(|deadline| deadline <= now);
                if !due {
                    continue;
                }
                let title_before = leaf.session.window_title().map(str::to_owned);
                let finished = leaf
                    .session
                    .finish_synchronized_update(now)
                    .context("finish timed-out DEC 2026 synchronized update")?;
                chrome_changed |= leaf.session.window_title() != title_before.as_deref();
                active_finished |= index == active && finished;
            }
        }
        if chrome_changed {
            self.window.set_title(&self.display_title());
            self.refresh_chrome();
        }
        if active_finished {
            self.publish_pty_drain_frame(now, false)?;
        }
        Ok(())
    }

    fn reset_cursor_blink(&mut self, now: Instant) -> bool {
        let changed = self.cursor_blink.reset(now);
        self.renderer
            .set_cursor_blink_visible(self.cursor_blink.visible());
        changed
    }

    fn set_cursor_focus(&mut self, focused: bool, now: Instant) {
        // The one place window focus changes hands — both `Focused(true)` and
        // `Focused(false)` arrive here — so the strip's copy is recorded here
        // too rather than in a second listener that could fall out of step.
        self.window_focused = focused;
        // M142: a window that has lost the keyboard has lost the pointer's
        // attention too, and a tip left floating over a background window is a
        // label on something you are no longer looking at.
        if !focused {
            self.tooltip.hide();
            self.layout_peek.hide();
        }
        self.cursor_blink.set_focused(focused, now);
        self.renderer.set_window_focused(focused);
        self.renderer
            .set_cursor_blink_visible(self.cursor_blink.visible());
    }

    fn advance_cursor_blink_if_due(&mut self, now: Instant) -> Result<()> {
        if !self.cursor_blink.advance(now) {
            return Ok(());
        }
        self.renderer
            .set_cursor_blink_visible(self.cursor_blink.visible());
        self.publish_frame(FrameTrigger {
            occurred_at: now,
            source: FrameSource::Expose,
        })
    }

    /// Redraw the tab strip if anything in a mark slot has moved.
    ///
    /// Modelled on [`Self::advance_cursor_blink_if_due`] and for the same
    /// reason: the strip is rebuilt only when a channel has actually changed,
    /// so a window with nothing running costs nothing at all. What decides
    /// "changed" here is the animation itself — a breath and a spin move on
    /// every frame they are alive, and neither moves at all once its session
    /// stops working.
    fn advance_strip_animation(&mut self, now: Instant) -> Result<()> {
        let active = self.active_tab;
        let window_is_focused = self.window_focused;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let tab_is_active = index == active;
            // Unread is counted per shell, against that shell's own revision
            // counter — two panes of one tab each have their own idea of how far
            // the user has read. The tab's badge is the aggregate of these
            // (D34), computed where the badge is drawn rather than tallied into
            // a second place that could drift.
            for (_, leaf) in tab.leaves_mut() {
                leaf.last_seen_revision = seen_revision(
                    leaf.last_seen_revision,
                    leaf.session.published_revision(),
                    tab_is_active,
                );
                // Watching is consuming: a latch that arrives on the tab the user
                // is already reading has been answered by the reading. Both latches
                // go together because `bt-term` retires them together, and because
                // "the user has seen this" is one fact and not two.
                if attention_is_consumed(tab_is_active, window_is_focused) {
                    leaf.session.clear_attention();
                }
            }
        }
        let motion = self.motion;
        let palette = bt_render::chrome_palette();
        let hovered = self.hovered_tab();
        let mut owes_frame = false;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            // A new progress reading starts the arc easing toward it. This runs
            // for every tab, active or not: a background download's ring has to
            // keep reporting, and its tab is exactly the one the user cannot
            // otherwise see.
            tab.sync_ring(now);
            // A pinned tab holds its pin open; an unpinned one offers it only
            // while you are on the tab (mock-up 324, 347-349).
            tab.pin_reveal.retarget(
                f32::from(u8::from(tab.pinned || hovered == Some(index))),
                now,
                motion,
            );
            // The reveal has to be *compared*, not merely sampled: `tab_owes_frame`
            // asks what would be drawn against what was drawn, and a width that
            // nothing compares would animate without ever scheduling a present.
            // Quantised to the 1/255 the sprite's own opacity resolves to, so a
            // tween settling in the last thousandth does not owe a frame forever.
            let drawn = tab.drawn_pin_reveal(now, motion);
            if tab.last_drawn_pin_reveal != Some(drawn) {
                tab.last_drawn_pin_reveal = Some(drawn);
                owes_frame = true;
            }
            // The tab in hand is driven by the pointer and never by this clock,
            // so it is deliberately not passed here: a settle or a FLIP is the
            // only thing on this axis that moves on its own.
            let offset = tab.drawn_offset(now, motion, None).round() as i32;
            let landed = (tab.landing.sample(now, motion).0 * 255.0).round() as u8;
            if tab.last_drawn_offset != Some(offset) || tab.last_drawn_landing != Some(landed) {
                tab.last_drawn_offset = Some(offset);
                tab.last_drawn_landing = Some(landed);
                owes_frame = true;
            }
            let showing = tab.mark_state(index == active, now, motion, &palette);
            if tab_owes_frame(tab.last_drawn_mark, showing) {
                tab.last_drawn_mark = Some(showing);
                owes_frame = true;
            }
        }
        // The `˅` is the strip's own and belongs to no tab, so it settles its
        // debt outside the loop — but on exactly the same terms: the angle that
        // would be *drawn*, which is the quantized one, against the angle that
        // was. Comparing the raw fraction instead would owe a frame on every
        // wake-up of the 140ms, including the long tail where the mark does not
        // change at all.
        let turning = marks::ChromeMark::chevron(self.chevron_turn.sample(now, motion).0);
        if tab_owes_frame(self.last_drawn_chevron, turning) {
            self.last_drawn_chevron = Some(turning);
            owes_frame = true;
        }
        // The dock box's fade settles its debt on the same terms as the pin's:
        // the opacity that would be *drawn*, quantised to the 1/255 a layer's
        // alpha resolves to, against the one that was. It is read here rather
        // than inside the overlay build because a debt has to be noticed by the
        // thing that decides whether to build at all.
        let faded = self.drawn_dock_reveal(now, motion);
        if self.last_drawn_dock_reveal != faded {
            self.last_drawn_dock_reveal = faded;
            owes_frame = true;
        }
        if !owes_frame {
            return Ok(());
        }
        // The strip decides for itself whether anything visibly moved. An
        // animation that is running but landed on the same pixels this frame —
        // a tween rounding to the same thousandth, a breath at the flat top of
        // its curve — still owes the *next* frame, which the deadline below
        // provides, but it does not owe a present now.
        if !self.refresh_chrome() {
            return Ok(());
        }
        self.publish_frame(FrameTrigger {
            occurred_at: now,
            source: FrameSource::Expose,
        })
    }

    /// When the tab strip next needs waking, or `None` when nothing is moving.
    ///
    /// `None` is the important half: it is what lets `about_to_wait` fall back
    /// to `ControlFlow::Wait` and the process go genuinely idle. A strip with
    /// no working session, no indeterminate ring and no tween in flight asks
    /// for no wake-ups at all, which is why this is a deadline rather than a
    /// standing 60fps loop.
    fn strip_animation_deadline(&self, now: Instant) -> Option<Instant> {
        let motion = self.motion;
        let tabs_moving = self.tabs.iter().any(|tab| {
            tab.mark_is_animating(now, motion)
                || tab.pin_is_animating(now, motion)
                || tab.flip.sample(now, motion).1
                || tab.landing.sample(now, motion).1
        });
        // The `˅` belongs to the strip and not to any tab, so a window with the
        // picker mid-turn and nothing else happening still has to be woken —
        // and, once the arrow lands, must stop being woken. Under reduced
        // motion this is never true: the turn has no frames to ask for.
        let chevron_turning = self.chevron_turn.sample(now, motion).1;
        // The dock box's fade is the one animation in this window that can be
        // running while the pointer is still: a drag that comes to rest over open
        // air still has 100ms of fade to finish, and nothing else would wake the
        // loop to draw it.
        let dock_fading = self
            .drop_preview
            .as_ref()
            .is_some_and(|shown| shown.reveal.sample(now, motion).1);
        (tabs_moving || chevron_turning || dock_fading).then(|| now + STRIP_ANIMATION_FRAME)
    }

    /// The dock box's opacity as the overlay would draw it, quantised to the
    /// 1/255 a layer's alpha resolves to — `None` when there is no box.
    fn drawn_dock_reveal(&self, now: Instant, motion: Motion) -> Option<u8> {
        self.drop_preview.as_ref().map(|shown| {
            let (reveal, _) = shown.reveal.sample(now, motion);
            (reveal.clamp(0.0, 1.0) * 255.0).round() as u8
        })
    }

    /// What every tab hangs off its trailing end, in strip order.
    ///
    /// The clock is read once, here, so the whole strip lays out against a single
    /// instant — sampling per tab would let two tabs in the same frame disagree
    /// about what time it is, and a reveal is a function of time.
    fn tab_trailers(&self, now: Instant) -> Vec<seats::TabTrailer> {
        let motion = self.motion;
        self.tabs
            .iter()
            .map(|tab| seats::TabTrailer {
                pinned: tab.pinned,
                reveal: tab.pin_reveal.sample(now, motion).0,
            })
            .collect()
    }

    /// Which tab the pointer is on. Every trailing control belongs to its tab,
    /// so hovering the `×` or the pin is still hovering the tab — the mock-up's
    /// `.tab:hover .pin` is a descendant rule and holds the pin open while the
    /// pointer is anywhere inside.
    fn hovered_tab(&self) -> Option<usize> {
        match self.seat_pointer.hover {
            Some(
                seats::ChromeTarget::Tab(index)
                | seats::ChromeTarget::TabClose(index)
                | seats::ChromeTarget::TabPin(index),
            ) => Some(index),
            _ => None,
        }
    }

    fn finish_resize_if_quiescent(&mut self, now: Instant) -> Result<()> {
        let mut active_finished = false;
        let active = self.active_tab;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            // Per leaf: each shell runs its own ConPTY resize transaction and
            // each PSReadLine holds its own anchor, so quiescence is reached one
            // shell at a time.
            for (_, leaf) in tab.leaves_mut() {
                if !leaf
                    .session
                    .finish_resize_if_quiescent(now)
                    .context("finish ConPTY resize transaction")?
                {
                    continue;
                }
                // `[Console]::CursorLeft/Top` in the PSReadLine handler makes ConPTY ask the terminal
                // `CSI 6 n`. Pay the coalesced repair only after the final resize request *and* every
                // child byte it caused have been quiet. A new geometry event re-opens the transaction,
                // so a divider storm cannot install an intermediate commit's still-moving cursor.
                let shell_input_region_open = leaf.session.shell_input_region_open();
                if let Some(reanchor_input) = take_psreadline_resize_reanchor_input(
                    &mut leaf.pending_psreadline_resize_reanchor,
                    shell_input_region_open,
                ) && let Some(pty) = leaf.pty.as_mut()
                {
                    pty.write(reanchor_input)
                        .context("request PSReadLine anchor repair after resize quiescence")?;
                }
                active_finished |= index == active;
            }
        }
        if active_finished {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    /// Carry a freshly solved grid to the child and to our own grid — or, when the retained
    /// typed-input policy is enabled and the shell is holding input, to neither.
    ///
    /// The seat rectangle has already moved by the time this is called (`resolve_seat_layout` is
    /// what produced `next_grid`), so a drag stays exactly as responsive as it is today. What is
    /// deferred is the pair: the ruling forbids desynchronizing our grid from ConPTY's, so under
    /// the gate they both stay at the old width and `flush_pending_pty_resize` moves them together.
    /// A grid that is wider than its seat is already the ordinary case for the renderer — the seat
    /// viewport scissors it — and it is the case a divider drag produces on every frame.
    fn schedule_grid_change(
        &mut self,
        next_grid: GridSize,
        physical: PhysicalSize<u32>,
        observed_at: Instant,
        context: &'static str,
    ) -> Result<()> {
        let deferred = self.typed_input_defers_resize();
        let active = self.active_tab;
        let conpty_grid = self.tabs[active].conpty_grid;
        let grid = self.tabs[active].grid;
        let Some(reflow) = plan_grid_change(
            &mut self.tabs[active].pending_pty_resize,
            next_grid,
            conpty_grid,
            grid,
            physical,
            observed_at,
            deferred,
        ) else {
            return Ok(());
        };
        self.session
            .resize(
                nonzero_u32(reflow.columns.get()),
                nonzero_u32(reflow.rows.get()),
            )
            .context(context)?;
        self.grid = reflow;
        Ok(())
    }

    /// Carry a new solve to the panes the user is not typing in.
    ///
    /// [`Self::schedule_grid_change`] speaks for the focused leaf, where the
    /// typed-input gate lives. That gate exists to keep a shell that is holding
    /// a half-typed line from being reflowed under the user's hands — a thing
    /// that can only be true of the pane with the keyboard. The others have no
    /// typed input to protect, so their grids follow the solver at once, actor
    /// then ConPTY, in the order every other resize path uses.
    ///
    /// Without this a split pane keeps the width it was born with: the window
    /// resizes, the divider moves, and one pane's shell goes on believing it has
    /// the columns it had at spawn.
    fn resize_unfocused_leaves(&mut self) -> Result<()> {
        let focused = self.focused_leaf;
        let scale = self.renderer.metrics().scale_factor as f32;
        let bodies: Vec<(bt_layout::SeatId, bt_render::SeatViewport)> = self
            .seats
            .terminals()
            .into_iter()
            .filter(|seat| *seat != focused)
            .filter_map(|seat| {
                seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)
                    .map(|body| (seat, body))
            })
            .collect();
        let active = self.active_tab;
        for (seat, body) in bodies {
            let next_grid = self
                .renderer
                .metrics()
                .grid_for_pixels(body.width, body.height);
            let physical = PhysicalSize::new(body.width, body.height);
            let Some(leaf) = self.tabs[active].sessions.get_mut(&seat) else {
                continue;
            };
            if next_grid == leaf.grid && next_grid == leaf.conpty_grid {
                continue;
            }
            leaf.session
                .resize(
                    nonzero_u32(next_grid.columns.get()),
                    nonzero_u32(next_grid.rows.get()),
                )
                .context("resize an unfocused pane's terminal actor for a seat layout change")?;
            leaf.grid = next_grid;
            let shell_input_region_open = leaf.session.shell_input_region_open();
            if let Some(pty) = leaf.pty.as_mut() {
                pty.resize(pty_size(next_grid, physical))
                    .context("resize an unfocused pane's ConPTY")?;
            }
            replace_psreadline_resize_reanchor_debt(
                &mut leaf.pending_psreadline_resize_reanchor,
                shell_input_region_open,
            );
            leaf.conpty_grid = next_grid;
        }
        Ok(())
    }

    /// Is a PTY resize deferred right now?
    ///
    /// This is the single policy convergence point. With `TYPED_INPUT_RESIZE_DEFERRAL` off it is
    /// always false. If that bit is restored, a child holding typed input defers as before, while
    /// `BT_PROBE_INPUT` — which spawns no child — still reflows immediately.
    fn typed_input_defers_resize(&self) -> bool {
        typed_input_resize_deferral_active(
            TYPED_INPUT_RESIZE_DEFERRAL,
            self.pty.is_some(),
            self.session.typed_shell_input_live(),
        )
    }

    fn flush_pending_pty_resize(&mut self, now: Instant) -> Result<Option<Instant>> {
        let deferred = self.typed_input_defers_resize();
        let (pending, wake_deadline) =
            service_pending_pty_resize(&mut self.pending_pty_resize, now, deferred);
        let Some(pending) = pending else {
            return Ok(wake_deadline);
        };
        // The deferred local reflow lands here, immediately before the child hears the same size,
        // in the same order the undeferred path uses (actor first, then ConPTY, then the vendor
        // reconcile). When nothing was deferred this is a no-op: our grid already moved at the
        // `Resized` that scheduled this.
        let reflowed = pending.grid != self.grid;
        if reflowed {
            self.session
                .resize(
                    nonzero_u32(pending.grid.columns.get()),
                    nonzero_u32(pending.grid.rows.get()),
                )
                .context("resize terminal actor for a released ConPTY resize")?;
            self.grid = pending.grid;
            self.sync_math_layout_key();
            self.pending_resize_present = Some(pending.grid);
        }
        // Snapshot the OSC 133 phase before resize reconciliation mutates terminal geometry. This
        // is broader than the typed-input gate on purpose: an empty, already-printed prompt caches
        // the same PSReadLine anchor and needs the same post-resize repair. Its 2.4.x handler is
        // output-free for an empty buffer; using InvokePrompt there abandons old wrapped rows on
        // every committed divider stop (the real-ConPTY chain probe pins that distinction).
        let shell_input_region_open = self.session.shell_input_region_open();
        if let Some(pty) = self.pty.as_mut() {
            pty.resize(pty_size(pending.grid, pending.physical))
                .context("commit coalesced final ConPTY resize")?;
        }
        // Replace, rather than accumulate, the current transaction's repair debt. The send happens
        // in `finish_resize_if_quiescent`, after ConPTY output has also been silent; a closed input
        // region records no debt and therefore still writes exactly zero private bytes.
        replace_psreadline_resize_reanchor_debt(
            &mut self.pending_psreadline_resize_reanchor,
            shell_input_region_open,
        );
        self.conpty_grid = pending.grid;
        // The quiet boundary is also where a resize *ends*, so it is the
        // meaningful change §5.1 asks the session write to be debounced behind.
        // Marking it on every intermediate `Resized` would turn one drag of a
        // window corner into a hundred disk writes.
        self.mark_session_dirty(now);
        let reconciled = self.session.mark_pty_resize_requested_at(
            nonzero_u32(pending.grid.columns.get()),
            nonzero_u32(pending.grid.rows.get()),
            now,
        );
        if reconciled || reflowed {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Resize,
            })?;
        }
        Ok(wake_deadline)
    }

    /// The terminal pane the pointer is inside, its seat-local position, and the
    /// frame it last drew.
    ///
    /// The whole of per-seat hit routing. Every pointer question below used to
    /// be asked of one frame in one rectangle, because there was one of each;
    /// with a fleet, "which cell is under the pointer" has no answer until you
    /// have said *which pane*, and the pointer's own coordinates are what say
    /// it. Deliberately not the focused leaf: hovering a link in the pane you
    /// are not typing in must underline that pane's link, and answering from the
    /// focused pane's cells would underline a cell the pointer is nowhere near.
    ///
    /// `None` when the pointer is over chrome, over a non-terminal pane, or over
    /// a pane that has not drawn yet — all three being "there is no cell here",
    /// which is exactly what the callers already do nothing about.
    fn pane_hit_context(
        &self,
    ) -> Option<(bt_layout::SeatId, PhysicalPosition<f64>, &ViewportFrame)> {
        let position = self.pointer_position?;
        let seat = seats::pane_at(&self.seat_layout, position.x, position.y)?;
        let leaf = self.sessions.get(&seat)?;
        let frame = leaf.last_presented_frame.as_ref()?;
        // The pane's *body*, not its seat: a pane with a head draws its grid
        // below that head, and a pointer measured from the seat's corner would
        // be off by the head's height on every pane that wears one.
        let scale = self.renderer.metrics().scale_factor as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        if position.x < f64::from(body.x)
            || position.y < f64::from(body.y)
            || position.x >= f64::from(body.x + body.width)
            || position.y >= f64::from(body.y + body.height)
        {
            return None;
        }
        Some((
            seat,
            PhysicalPosition::new(
                position.x - f64::from(body.x),
                position.y - f64::from(body.y),
            ),
            frame,
        ))
    }

    fn frame_hit(&self) -> Option<bt_render::GridHit> {
        let (_, position, frame) = self.pane_hit_context()?;
        self.renderer
            .metrics()
            .hit_test_frame(frame, position.x, position.y)
    }

    fn math_hit(&self) -> Option<MathHit> {
        let (_, position, frame) = self.pane_hit_context()?;
        self.renderer.math_hit_test(frame, position.x, position.y)
    }

    fn hyperlink_hit(&self, hit: bt_render::GridHit) -> Option<HyperlinkHit> {
        // Asked of the pane the pointer is in, so the link that lights up is the
        // link under the hand.
        let (_, _, frame) = self.pane_hit_context()?;
        frame.hyperlink_at(hit.row, hit.column)
    }

    /// The file a Ctrl+click at `hit` may hand to the system viewer (preview matrix §4, "leave this
    /// product"). Never path-*looking* text: the verb has always required that a worker actually
    /// opened and decoded the file, so that a misdetected word cannot launch anything.
    ///
    /// The cells are the frame's own — the very cells wearing the underline that promised the
    /// picture — so the verb and the mark cannot reach different text. Verification is the scan's
    /// `verified` (the decoration worker's record of this file) or the peek's own cache entry, which
    /// is the same worker and the same decoder reached by hovering rather than by detection.
    fn local_image_path_hit(&self, hit: bt_render::GridHit) -> Option<PathBuf> {
        let reference = self.frame_image_references.at(hit)?;
        (reference.verified
            || matches!(
                self.peek_cache
                    .get(&normalized_local_image_path_key(&reference.path)),
                Some(PeekCacheEntry::Ready { .. })
            ))
        .then(|| reference.path.clone())
    }

    /// The verified image reference the pointer is currently standing on, if any — the cells whose
    /// resting dots become a solid underline for as long as the pointer is there.
    ///
    /// Resolved from the frame's own scan per publish rather than remembered in hover state, because
    /// it is a pure function of "where is the pointer" and "what does this frame draw there", and
    /// both can change without a pointer event: a decode landing turns plain text into an underlined
    /// reference under a pointer that never moved.
    fn hovered_image_reference(&self) -> Option<bt_term::FrameImageReference> {
        let hit = self.frame_hit()?;
        self.frame_image_references
            .at(hit)
            .filter(|reference| reference.verified)
            .cloned()
    }

    /// Repaint when the pointer has moved onto or off a verified reference, so the solid underline
    /// arrives with the pointer rather than with the next unrelated frame.
    ///
    /// The other direction — a decode landing under a pointer that never moved — needs nothing
    /// here: `apply_math_results` already republishes when a completion changed session state, and
    /// the compose step asks the session afresh.
    fn refresh_image_reference_underline(&mut self) -> Result<()> {
        if self.hovered_image_reference() == self.underlined_image_reference {
            return Ok(());
        }
        self.publish_interaction_frame()
    }

    /// The image a hover at `hit` may preview, from any shape the screen can offer it in.
    ///
    /// The frame's own scan comes first, and it already carries every shape whose file can be named
    /// from what is drawn: a printed path, an OSC 7 relative form, a bare `file://` URI, and the
    /// target of an OSC 8 link whose visible text names no file at all. It is the same list the
    /// underline is painted from, which is what makes "you can peek exactly what is marked" a fact
    /// about one list rather than an agreement between two. Verification is not required here: a
    /// hover is how a picture nobody has opened yet gets opened.
    ///
    /// Last comes the one shape that names no file: an OSC 1337 payload, hovered over the
    /// `[image]` placeholder the adapter wrote for it. It is asked last because it is the only
    /// source that cannot be re-read — where a path and a placeholder somehow shared a cell, the
    /// text the pointer was actually put on is still what wins.
    ///
    /// The complement of inline admission is checked once, here, before any source is consulted —
    /// a link that happens to lie across a banded content point does not smuggle a second
    /// presentation of it.
    fn peek_target(&self, hit: bt_render::GridHit) -> Option<PeekSubject> {
        let anchor = self
            .last_presented_frame
            .as_ref()?
            .anchor_at(hit.row, hit.column, Bias::Before)
            .ok()??;
        if !self.session.peek_admits_at(&anchor) {
            return None;
        }
        self.frame_image_references
            .at(hit)
            .map(|reference| PeekSubject::from_path(reference.path.clone()))
            .or_else(|| {
                self.session
                    .inline_image_payload_peek_at(&anchor)
                    .map(PeekSubject::from_content_key)
            })
    }

    /// Present a pure peek-overlay change. The overlay lives beside the frame, not inside it, so
    /// `redraw` would find nothing queued and skip: when nothing newer is pending, the frame that
    /// is already on screen re-enters the slot; a queued newer frame carries the overlay along on
    /// its own redraw.
    fn present_peek_overlay(&mut self, overlay: Option<PeekImageOverlay>) -> Result<()> {
        if !self.renderer.set_peek_overlay(overlay) {
            return Ok(());
        }
        // Not while a resize present is outstanding: that gate admits only the newly projected
        // grid, and the frame on screen is the previous one. A repaint is already owed to the
        // resize, and it carries the renderer-side overlay state with it.
        if self.pending_resize_present.is_none()
            && self.pending_frames.pending_frame().is_none()
            && let Some(frame) = self.last_presented_frame.clone()
        {
            self.pending_frames
                .publish(
                    frame,
                    FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::Expose,
                    },
                )
                .context("re-present the on-screen frame for a peek overlay change")?;
        }
        self.window.request_redraw();
        Ok(())
    }

    /// Drop peek hover state and hide the flyout. Idempotent; used by every dismiss gesture
    /// (pointer off the span, pointer left, wheel, click, any key).
    fn dismiss_peek(&mut self) -> Result<()> {
        self.peek_hover.clear();
        self.present_peek_overlay(None)
    }

    fn activate_peek_if_due(&mut self, now: Instant) -> Result<()> {
        if let Some(candidate) = self.peek_hover.activate_if_due(now) {
            self.show_or_request_peek(&candidate)?;
        }
        Ok(())
    }

    /// Resolve the flyout for a settled hover: decode the subject if it is new, resample the decode
    /// into the box this viewport will draw it in if that raster is not the one already held, and
    /// present when display-sized pixels are in hand. Each miss is one worker round trip and the
    /// completion re-enters here, so the event thread neither decodes nor resamples.
    fn show_or_request_peek(&mut self, candidate: &PeekCandidate) -> Result<()> {
        let cache_key = candidate.subject.key.clone();
        let (content_key, native_rgba, native_width_px, native_height_px) =
            match self.peek_cache.get(&cache_key) {
                Some(PeekCacheEntry::Ready {
                    key,
                    rgba,
                    width_px,
                    height_px,
                }) => (key.clone(), Arc::clone(rgba), *width_px, *height_px),
                // A failed decode stays silent: the terminal text is the honest surface, and the
                // negative entry keeps hovers from re-hitting the disk.
                Some(PeekCacheEntry::Pending) | Some(PeekCacheEntry::Failed) => return Ok(()),
                None => {
                    // Nothing to read: a stream payload is cached when its decode lands or never,
                    // and the session only names one whose decode already succeeded, so a miss
                    // here is a hover that arrived first. The next one finds it.
                    let Some(path) = candidate.subject.path.clone() else {
                        return Ok(());
                    };
                    if !self.math_worker_running {
                        return Ok(());
                    }
                    if self
                        .math_worker
                        .tasks
                        .send(MathWorkerRequest::PeekImage {
                            tab_id: self.id,
                            path,
                        })
                        .is_ok()
                    {
                        self.peek_cache.insert(cache_key, PeekCacheEntry::Pending);
                    }
                    return Ok(());
                }
            };
        // The renderer owns the box; a pane too small to host the flyout shows none, and nothing
        // is resampled for it.
        let Some((display_width_px, display_height_px)) = self
            .renderer
            .peek_thumbnail_extent(native_width_px, native_height_px)
        else {
            return Ok(());
        };
        let target: PeekThumbnailTarget = (content_key, display_width_px, display_height_px);
        if let Some(thumbnail) = self.peek_thumbnail.as_ref()
            && thumbnail.matches(&target)
        {
            let overlay = thumbnail.overlay(candidate.pointer);
            return self.present_peek_overlay(Some(overlay));
        }
        if self.peek_thumbnail_pending.as_ref() == Some(&target) || !self.math_worker_running {
            return Ok(());
        }
        if self
            .math_worker
            .scale_tasks
            .send(ScaleWorkerRequest::Peek {
                tab_id: self.id,
                task: peek_scale_task(&target, native_rgba, native_width_px, native_height_px),
            })
            .is_ok()
        {
            self.peek_thumbnail_pending = Some(target);
        }
        Ok(())
    }

    /// Remember a decoration-worker decode in the peek cache, under the identity the peek asks by:
    /// the decoder's content key for an OSC 1337 payload, the normalized path for a named file.
    ///
    /// Two reasons, one seam. A stream payload is the one image the peek cannot go and fetch — the
    /// bytes were in the stream, the session decoded them once, and nothing else will ever ask for
    /// them again — so catching it here is the only way the `[image]` placeholder can peek at all.
    /// A named file *can* be re-read, but under the 2026-08-04 verification ruling the session has
    /// just had a worker open, size-check, format-check and decode it in order to earn the resting
    /// underline; letting that decode reach the flyout's cache is what makes the promised peek
    /// appear at once instead of after a second read of the same file. The ruling names that as the
    /// beneficial side effect to preserve, and this is where it is preserved.
    ///
    /// One file still has one entry: the key is `normalized_local_image_path_key`, the very key
    /// `PeekSubject::from_path` computes, so this fills the entry the peek would have created
    /// rather than adding a second one that could disagree.
    fn remember_decode_for_peek(
        &mut self,
        task: &bt_term::InlineImageTask,
        decoded: Option<&bt_term::DecodedInlineImage>,
    ) {
        let Some(decoded) = decoded else {
            return;
        };
        let cache_key = peek_cache_key_for_decode(&task.source, decoded);
        self.peek_cache.insert(
            cache_key,
            PeekCacheEntry::Ready {
                key: decoded.key.clone(),
                rgba: Arc::clone(&decoded.rgba),
                width_px: decoded.width_px,
                height_px: decoded.height_px,
            },
        );
    }

    /// Record a peek decode outcome and, when the hover is still settled on that path, show the
    /// flyout at the settle pointer.
    fn complete_peek_image(
        &mut self,
        path: PathBuf,
        result: std::result::Result<bt_term::DecodedInlineImage, bt_term::InlineImageDecodeError>,
    ) -> Result<()> {
        let cache_key = normalized_local_image_path_key(&path);
        let preview_matches = self
            .preview_image
            .as_ref()
            .is_some_and(|preview| normalized_local_image_path_key(&preview.path) == cache_key);
        match result {
            Ok(decoded) => {
                self.peek_cache.insert(
                    cache_key.clone(),
                    PeekCacheEntry::Ready {
                        key: decoded.key,
                        rgba: decoded.rgba,
                        width_px: decoded.width_px,
                        height_px: decoded.height_px,
                    },
                );
                if let Some(active) = self.peek_hover.active.clone()
                    && active.subject.key == cache_key
                {
                    self.show_or_request_peek(&active)?;
                }
            }
            Err(error) => {
                self.peek_cache
                    .insert(cache_key.clone(), PeekCacheEntry::Failed);
                if preview_matches && let Some(preview) = self.preview_image.as_mut() {
                    preview.failure = Some(format!("Preview failed: {error}"));
                }
            }
        }
        if preview_matches {
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take delivery of the flyout's display-sized raster. Only the question still outstanding is
    /// answered here: an earlier size arriving after the viewport moved on leaves the newer request
    /// in flight rather than asking for it twice.
    fn complete_peek_scale(&mut self, scaled: bt_term::ScaledInlineImage) -> Result<()> {
        let delivered: PeekThumbnailTarget = (
            scaled.content_key.clone(),
            scaled.width_px,
            scaled.height_px,
        );
        if self.peek_thumbnail_pending.as_ref() == Some(&delivered) {
            self.peek_thumbnail_pending = None;
        }
        self.peek_thumbnail = Some(PeekThumbnail::from_scaled(scaled));
        if let Some(active) = self.peek_hover.active.clone() {
            self.show_or_request_peek(&active)?;
        }
        Ok(())
    }

    fn complete_preview_scale(&mut self, scaled: bt_term::ScaledInlineImage) -> Result<()> {
        let Some(preview) = self.preview_image.as_mut() else {
            return Ok(());
        };
        if !preview.accept_scaled(scaled) {
            return Ok(());
        }
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    fn activate_hyperlink_hover_if_due(&mut self, now: Instant) -> Result<()> {
        if self.hyperlink_hover.activate_if_due(now) {
            self.publish_interaction_frame()?;
        }
        Ok(())
    }

    fn pointer_left(&mut self) -> Result<()> {
        self.pointer_position = None;
        // Deliberately *not* a drag cancel, and the reason is measurable rather
        // than stylistic: winit takes the Win32 mouse capture on button-down
        // (`capture_mouse`, its `WM_LBUTTONDOWN` arm), so a held drag keeps
        // receiving motion outside the window and is guaranteed its own
        // button-up. `CursorLeft` here means the pointer crossed the client
        // rect, which during a drag is an ordinary thing to do — the tab strip
        // runs to the window's top edge — and cancelling on it would throw a
        // reorder away for a pixel of overshoot. K129's real cancel is capture
        // loss, and the only capture loss winit surfaces is losing the window.
        // The overlay's own hover goes with it: a `×` still lit after the
        // pointer has left the window is a button claiming to be under a
        // pointer that is not there.
        let settings_hover_cleared = self.settings.set_hover(None);
        if (self.seat_pointer.hover.take().is_some() || settings_hover_cleared)
            && self.refresh_chrome()
        {
            self.present_chrome_change()?;
        }
        self.dismiss_peek()?;
        let hyperlink_changed = self.hyperlink_hover.clear();
        if self.math_hover_anchor.is_some() {
            self.math_hover_clear_at = Some(Instant::now() + Duration::from_millis(500));
        }
        if hyperlink_changed {
            self.publish_interaction_frame()?;
        }
        // The pointer is gone, so `hovered_image_reference` now answers `None`: any solid underline
        // still on screen must fall back to its resting dots.
        self.refresh_image_reference_underline()?;
        Ok(())
    }

    fn activate_hyperlink(&mut self, hyperlink: HyperlinkHit) -> Result<()> {
        match hyperlink_activation(true, true, &hyperlink.uri) {
            HyperlinkActivation::None => {}
            HyperlinkActivation::Open => {
                let result = window_hwnd(&self.window).and_then(|hwnd| {
                    bt_platform::shell_execute(hwnd, &hyperlink.uri)
                        .map_err(|error| anyhow!(error))
                        .context("open HTTP hyperlink in the system browser")
                });
                if let Err(error) = result {
                    eprintln!("recoverable hyperlink open failure: {error:#}");
                }
            }
            HyperlinkActivation::Blocked => {
                self.hyperlink_hover.show_blocked(hyperlink);
                self.publish_interaction_frame()?;
            }
        }
        Ok(())
    }

    fn activate_local_image_path(&self, path: &std::path::Path) {
        let result = window_hwnd(&self.window).and_then(|hwnd| {
            bt_platform::open_local_file(hwnd, path)
                .map_err(|error| anyhow!(error))
                .context("open decoded local image in the system viewer")
        });
        if let Err(error) = result {
            eprintln!("recoverable local image open failure: {error:#}");
        }
    }

    fn update_math_hover(&mut self, now: Instant) -> Result<Option<MathHit>> {
        let hit = self.math_hit();
        if let Some(hit) = hit.as_ref() {
            self.math_hover_clear_at = None;
            if self.math_hover_anchor.as_ref() != Some(&hit.anchor) {
                self.math_hover_anchor = Some(hit.anchor.clone());
                if self.session.set_math_hover(Some(&hit.anchor)) {
                    self.publish_interaction_frame()?;
                }
            }
        } else if self.math_hover_anchor.is_some() && self.math_hover_clear_at.is_none() {
            self.math_hover_clear_at = Some(now + Duration::from_millis(500));
        }
        Ok(hit)
    }

    fn clear_math_hover_if_due(&mut self, now: Instant) -> Result<()> {
        if self
            .math_hover_clear_at
            .is_none_or(|deadline| now < deadline)
        {
            return Ok(());
        }
        self.math_hover_clear_at = None;
        self.math_hover_anchor = None;
        if self.session.set_math_hover(None) {
            self.publish_interaction_frame()?;
        }
        Ok(())
    }

    fn copy_math_latex(&mut self, anchor: &MathBlockAnchor) {
        let Some(source) = self.session.math_source(anchor) else {
            return;
        };
        let result = window_hwnd(&self.window).and_then(|hwnd| {
            bt_platform::set_clipboard_text(hwnd, source)
                .map_err(|error| anyhow!(error))
                .context("copy original LaTeX source to clipboard")
        });
        recoverable_clipboard_write(result, "formula copy");
    }

    fn apply_math_context_menu_result(&mut self) {
        let Some(result) = self.math_context_menu.take_result() else {
            return;
        };
        let anchor = self.pending_math_context_anchor.take();
        self.mouse_route = None;
        match (result, anchor) {
            (Ok(true), Some(anchor)) => self.copy_math_latex(&anchor),
            (Ok(true), None) => {
                eprintln!("recoverable formula context-menu result had no pending anchor");
            }
            (Ok(false), _) => {}
            (Err(error), _) => {
                eprintln!("recoverable formula context-menu failure: {error}");
            }
        }
    }

    fn publish_interaction_frame(&mut self) -> Result<()> {
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    fn clear_selection(&mut self) {
        self.session.set_view_selection(None);
        self.projection.set_selection(None);
    }

    fn return_to_live_for_input(&mut self) -> bool {
        let changed = self.session.view_selection().is_some()
            || self.projection.is_scrolled()
            || self.session.release_presentation_hold_for_user_input();
        self.clear_selection();
        self.projection.scroll_to_bottom();
        changed
    }

    fn send_user_input(
        &mut self,
        bytes: &[u8],
        context: &'static str,
        kind: UserInputKind,
    ) -> Result<()> {
        let view_changed = kind.returns_view_to_live() && self.return_to_live_for_input();
        // Typing into the stand-in shell is what makes it yours. From here on a
        // Restore appends beside it instead of clearing it away underneath you.
        if self.placeholder_tab == Some(self.tabs[self.active_tab].id) {
            self.placeholder_tab = None;
        }
        self.pending_keyboard_at = Some(Instant::now());
        if let Some(pty) = self.pty.as_mut() {
            pty.write(bytes).with_context(|| context)?;
        }
        if view_changed {
            self.publish_frame(FrameTrigger {
                occurred_at: self.pending_keyboard_at.unwrap_or_else(Instant::now),
                source: FrameSource::Keyboard,
            })?;
        }
        Ok(())
    }

    fn copy_selection(&mut self) -> Result<()> {
        let window = Arc::clone(&self.window);
        let active = self.active_tab;
        let leaf = self.tabs[active].focused_mut();
        if !copy_selection(&mut leaf.session, &mut leaf.projection, |text| {
            write_terminal_clipboard_text(&window, text)
        }) {
            return Ok(());
        }
        self.publish_interaction_frame()
    }

    fn copy_selection_on_release(&self) {
        let window = Arc::clone(&self.window);
        write_selection_text(&self.session, true, |text| {
            write_terminal_clipboard_text(&window, text)
        });
    }

    fn scroll_view(&mut self, rows: i32) -> Result<()> {
        let subpixels =
            i64::from(rows).saturating_mul(self.projection.cell_height_subpixels().get());
        self.projection.scroll_by_subpixels(subpixels);
        self.publish_interaction_frame()
    }

    fn begin_local_selection(&mut self, hit: bt_render::GridHit) -> Result<()> {
        self.dismiss_peek()?;
        let count = self
            .click_tracker
            .register(hit.row, hit.column, Instant::now());
        let local_image_path = self.local_image_path_hit(hit);
        let frame = self
            .last_presented_frame
            .as_ref()
            .context("missing frame for mouse hit")?;
        let hyperlink = frame.hyperlink_at(hit.row, hit.column);
        let open_hyperlink_on_release = self.modifiers.control_key() && hyperlink.is_some();
        let local_image_activation = local_image_activation(
            self.modifiers.control_key(),
            true,
            local_image_path.as_deref(),
        );
        // Local hits are clamped to a continuous frame, which supplies anchors for every grid cell.
        let (mode, origin, initial) = match count {
            2 => {
                let selection = frame
                    .word_selection(hit.row, hit.column)
                    .context("reject non-rectangular frame during word selection")?
                    .context("word selection hit has no anchor")?;
                (SelectionDragMode::Word, selection.clone(), Some(selection))
            }
            3 => {
                let selection = frame
                    .line_selection(hit.row)
                    .context("reject non-rectangular frame during line selection")?
                    .context("line selection hit has no anchor")?;
                (SelectionDragMode::Line, selection.clone(), Some(selection))
            }
            _ => {
                let start = frame
                    .anchor_at(hit.row, hit.column, Bias::Before)
                    .context("reject non-rectangular frame during anchor lookup")?
                    .context("selection hit has no start anchor")?;
                let end = frame
                    .anchor_at(hit.row, hit.column, Bias::After)
                    .context("reject non-rectangular frame during anchor lookup")?
                    .context("selection hit has no end anchor")?;
                (
                    SelectionDragMode::Linear,
                    ViewSelection {
                        start: start.clone(),
                        end,
                    },
                    None,
                )
            }
        };
        // A linear press begins a possible drag but owns no selection yet. Only movement creates
        // one, so click-no-drag cannot briefly feed copy-on-select or leave a zero-width selection.
        self.session.set_view_selection(initial);
        self.mouse_route = Some(MouseRoute::Local(SelectionDrag {
            mode,
            origin_row: hit.row,
            origin_column: hit.column,
            origin,
            hyperlink,
            open_hyperlink_on_release,
            local_image_activation,
        }));
        self.publish_interaction_frame()
    }

    fn extend_local_selection(&mut self, hit: bt_render::GridHit) -> Result<()> {
        let Some(MouseRoute::Local(drag)) = self.mouse_route.as_ref().cloned() else {
            return Ok(());
        };
        if matches!(drag.mode, SelectionDragMode::Linear)
            && hit.row == drag.origin_row
            && hit.column == drag.origin_column
        {
            return Ok(());
        }
        let frame = self
            .last_presented_frame
            .as_ref()
            .context("missing frame for mouse drag")?;
        let current = match drag.mode {
            SelectionDragMode::Linear => ViewSelection {
                start: frame
                    .anchor_at(hit.row, hit.column, Bias::Before)
                    .context("reject non-rectangular frame during drag anchor lookup")?
                    .context("drag hit has no start anchor")?,
                end: frame
                    .anchor_at(hit.row, hit.column, Bias::After)
                    .context("reject non-rectangular frame during drag anchor lookup")?
                    .context("drag hit has no end anchor")?,
            },
            SelectionDragMode::Word => frame
                .word_selection(hit.row, hit.column)
                .context("reject non-rectangular frame during word drag")?
                .context("word drag hit has no anchor")?,
            SelectionDragMode::Line => frame
                .line_selection(hit.row)
                .context("reject non-rectangular frame during line drag")?
                .context("line drag hit has no anchor")?,
        };
        let after_origin = (hit.row, hit.column) >= (drag.origin_row, drag.origin_column);
        self.session.set_view_selection(Some(if after_origin {
            ViewSelection {
                start: drag.origin.start,
                end: current.end,
            }
        } else {
            ViewSelection {
                start: current.start,
                end: drag.origin.end,
            }
        }));
        self.publish_interaction_frame()
    }

    fn pointer_moved(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        self.pointer_position = Some(position);
        // The overlay owns the pointer the way it owns the next click: no chrome
        // hover, no divider, no hyperlink, no peek settle behind the scrim.
        if let Some(layout) = self.settings_layout() {
            let hover = settings::hit(&layout, position.x, position.y);
            if self.settings.set_hover(Some(hover)) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        // The prompt is not modal either. Over its own box the buttons light up;
        // everywhere else the window carries on, because the terminal behind it
        // is still yours to use while the question stands.
        if let Some(layout) = self.restore_layout() {
            let over = restore::hit(&layout, position.x, position.y);
            if over.is_some() {
                if self.restore_prompt.set_hover(over) && self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
                return Ok(());
            }
            if self.restore_prompt.set_hover(None) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
        }
        // The picker is not modal, so it takes the pointer only where it is: over
        // its own box the rows answer, and everywhere else the window carries on.
        if let Some(layout) = self.profile_menu_layout() {
            let over = profiles::hit(&layout, position.x, position.y);
            if self.profile_menu.set_hover(over.flatten()) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            if over.is_some() {
                self.update_chrome_hover_target(None)?;
                return Ok(());
            }
        }
        // A divider drag owns the pointer outright: while one is in flight the
        // terminal hears nothing, which is the same rule an in-progress
        // selection drag already lives by.
        if self.drive_divider_drag(position)? {
            return Ok(());
        }
        // A press that has travelled past the drag threshold becomes a drag
        // (J112/J113), and a tab press on its way withdraws any activation it was
        // still holding back (J105). Both sources cross the same six pixels
        // through the same [`DragLatch`]; what differs is only what each press
        // was holding on to.
        //
        // J122 is upheld by position rather than by a flag: `drive_divider_drag`
        // has already returned above if a resize is in flight, so neither branch
        // below can be reached while one is — "one gesture owns the pointer at a
        // time", and the ordering is what says so.
        let scale = self.renderer.metrics().scale_factor;
        if self
            .tab_press
            .as_mut()
            .is_some_and(|press| press.travelled(position, scale))
        {
            let press = self.tab_press.expect("a press that travelled is a press");
            self.begin_tab_drag(press, position)?;
        } else if self
            .pane_press
            .as_mut()
            .is_some_and(|press| press.latch.travelled(position, scale))
        {
            let seat = self
                .pane_press
                .expect("a press that travelled is a press")
                .seat;
            self.begin_pane_drag(seat, position)?;
        }
        // A drag owns the pointer outright, exactly as a divider drag does:
        // hover, the peek flyout, the hyperlink underline and the terminal's own
        // selection all go quiet for the length of the gesture.
        if self.drive_drag(position)? {
            return Ok(());
        }
        self.update_chrome_hover(position)?;
        // Below every gesture that owns the pointer and beside the hover it
        // follows: the anchors under a drag, a divider or an open picker were
        // never reached, and each of those paths returned above having said so.
        // The peek first, and the tip only where the peek is not already
        // answering (§6). A tab that qualifies for neither is untouched by both.
        self.note_layout_peek(self.layout_peek_target_at(position))?;
        let anchor = self
            .tooltip_anchor_at(position)
            .filter(|anchor| !self.layout_peek_suppresses(*anchor));
        self.note_tooltip(anchor)?;
        // Everything below reads the pointer through `terminal_pointer` — one
        // correction, in one place, applied to hover, peek, selection and
        // protocol forwarding alike. Outside the seat it answers `None`, which
        // is exactly what a pointer outside the grid already answered, so the
        // clearing paths below run unchanged.
        let local = self.terminal_pointer();
        let position = local.unwrap_or(position);
        let now = Instant::now();
        let math_hit = self.update_math_hover(now)?;
        let hit = self.frame_hit();
        let hyperlink = hit
            .filter(|_| {
                math_hit.is_none() && !matches!(self.mouse_route, Some(MouseRoute::Local(_)))
            })
            .and_then(|hit| self.hyperlink_hit(hit));
        if self.hyperlink_hover.observe(hyperlink, now) {
            self.publish_interaction_frame()?;
        }
        self.refresh_image_reference_underline()?;
        let peek_path = hit
            .filter(|_| {
                math_hit.is_none() && !matches!(self.mouse_route, Some(MouseRoute::Local(_)))
            })
            .and_then(|hit| self.peek_target(hit));
        if self.peek_hover.observe(peek_path, position, now) {
            self.present_peek_overlay(None)?;
        }
        let Some(hit) = hit else {
            return Ok(());
        };
        if math_hit.is_some() || matches!(self.mouse_route, Some(MouseRoute::MathBlock)) {
            return Ok(());
        }
        if matches!(self.mouse_route, Some(MouseRoute::Local(_))) {
            return self.extend_local_selection(hit);
        }
        let frame = self
            .last_presented_frame
            .as_ref()
            .context("missing frame for forwarded mouse motion")?;
        let hit = live_viewport_mouse_hit(frame, hit);
        let modes = self.session.terminal_modes();
        if self.modifiers.shift_key() || modes.mouse_tracking == MouseTracking::Off {
            return Ok(());
        }
        let button = match self.mouse_route {
            Some(MouseRoute::Forward(button)) if modes.mouse_tracking != MouseTracking::Click => {
                button
            }
            None if modes.mouse_tracking == MouseTracking::Motion => {
                input::MouseProtocolButton::None
            }
            _ => return Ok(()),
        };
        let bytes = input::mouse_bytes(
            modes.sgr_mouse,
            button,
            input::MouseProtocolEvent::Motion,
            hit.row,
            hit.column,
            self.modifiers,
        );
        self.send_user_input(
            &bytes,
            "forward SGR mouse motion to PTY",
            UserInputKind::Mouse,
        )
    }

    /// Advance a divider drag. Returns whether the pointer was consumed.
    ///
    /// Every frame re-reads the split's slot from the current solve and asks
    /// `bt-layout::apply` for the ratio: the clamp, and the refusal when the
    /// clamp is unsatisfiable, are §2.4's and are not re-derived here. Red line
    /// L9 is upheld by the edit itself — `DragDivider`'s focus set is exactly
    /// that one split, so nothing rebalances mid-gesture.
    fn drive_divider_drag(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(drag) = self.divider_drag else {
            return Ok(false);
        };
        let Some(slot) = self
            .seats
            .split_slots(&self.seat_layout)
            .into_iter()
            .find(|slot| slot.id == drag.split)
        else {
            self.divider_drag = None;
            return Ok(false);
        };
        let along = match drag.dir {
            Axis::Row => position.x,
            Axis::Col => position.y,
        };
        let scale_ppm = seats::scale_ppm(self.renderer.metrics().dpi_milli().get());
        let Some((requested, usable)) = seats::requested_ratio(slot, scale_ppm, along) else {
            return Ok(true);
        };
        let metrics = self.seat_metrics();
        // A refusal, and a clamp that changed nothing, both mean "do not
        // re-solve": §2.4 rules that an infeasible drag has zero side effects
        // rather than writing a value the next solve will "correct", which
        // would dress a refusal up as a jitter.
        if self
            .seats
            .drag_divider(&metrics, drag.split, requested, usable)
            == Ok(true)
        {
            self.commit_seat_geometry()?;
        }
        Ok(true)
    }

    /// F71/T225: abandon a divider drag and put back the one value it moved.
    ///
    /// "Never mind, and no commit" — the same sentence Esc says to a tab drag,
    /// and it reaches here by the same two doors: the Esc key, and losing the
    /// window, which on Win32 is losing the mouse capture and is this platform's
    /// only `pointercancel` (F72). A drag that ends without either a button-up
    /// or a teardown would keep steering the layout from a pointer nobody is
    /// holding.
    ///
    /// The restore goes back through `Edit::DragDivider` rather than writing the
    /// ratio into the tree directly, and that is the point of it: §2.4's
    /// feasibility judgement and clamp are asked once more, so if the viewport
    /// changed mid-gesture the answer is the *current* legal value nearest the
    /// one we started from, rather than a ratio that was legal for a window that
    /// no longer exists. When nothing changed the clamp is idempotent and the
    /// origin comes back byte for byte.
    ///
    /// Zero side effects when there is nothing to undo: a press that never moved
    /// the ratio restores a value equal to the one already there, `drag_divider`
    /// reports no change, and no re-solve is asked for.
    fn cancel_divider_drag(&mut self) -> Result<bool> {
        let Some(drag) = self.divider_drag.take() else {
            return Ok(false);
        };
        self.seat_pointer.dragging = None;
        let usable = self
            .seats
            .split_slots(&self.seat_layout)
            .into_iter()
            .find(|slot| slot.id == drag.split)
            .map(|slot| slot.slot.extent(slot.dir) - bt_layout::DIVIDER);
        let metrics = self.seat_metrics();
        let restored = match usable {
            Some(usable) => {
                self.seats
                    .drag_divider(&metrics, drag.split, drag.origin, usable)
                    == Ok(true)
            }
            // The split is gone from the solve, so there is no ratio of its to
            // put back and nothing to re-solve for.
            None => false,
        };
        if restored {
            self.commit_seat_geometry()?;
        }
        self.apply_pointer_cursor();
        if let Some(position) = self.pointer_position {
            self.update_chrome_hover(position)?;
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Repaint the chrome when the pointer moves onto or off a divider, a close
    /// affordance or a collapsed bar.
    fn chrome_target_at(&self, position: PhysicalPosition<f64>) -> Option<seats::ChromeTarget> {
        let scale = self.renderer.metrics().scale_factor as f32;
        let width = self.renderer.presentation_geometry().swapchain_size.0 as f32;
        seats::hit_tab_chrome(
            width,
            scale,
            &self.tab_trailers(Instant::now()),
            self.active_tab,
            self.tab_scroll,
            position.x,
            position.y,
        )
        .or_else(|| seats::hit_window_chrome(width, scale, position.x, position.y))
        .or_else(|| {
            seats::hit_chrome(
                &self.seats,
                &self.seat_layout,
                scale,
                position.x,
                position.y,
            )
        })
    }

    fn update_chrome_hover(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        let hover = self.chrome_target_at(position);
        // `.pane:hover` is a second question about the same pointer, and it has
        // to be asked here rather than derived from `hover`: over a terminal's
        // body `hover` is `None`, because a terminal is not chrome, and that is
        // most of the pane.
        let pane = seats::pane_at(&self.seat_layout, position.x, position.y);
        self.update_chrome_hover_target_in_pane(hover, pane)
    }

    fn update_chrome_hover_target(&mut self, hover: Option<seats::ChromeTarget>) -> Result<()> {
        // A caller that has decided the pointer belongs to something floating
        // over the panes — an open picker, the restore prompt — is saying the
        // pointer is not in a pane either.
        self.update_chrome_hover_target_in_pane(hover, None)
    }

    fn update_chrome_hover_target_in_pane(
        &mut self,
        hover: Option<seats::ChromeTarget>,
        pane: Option<bt_layout::SeatId>,
    ) -> Result<()> {
        if self.seat_pointer.hover == hover && self.seat_pointer.pane_hover == pane {
            return Ok(());
        }
        self.seat_pointer.hover = hover;
        self.seat_pointer.pane_hover = pane;
        self.apply_pointer_cursor();
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The pointer wears the shape of what it is over: a resize arrow along a
    /// divider's axis (kept for the whole drag, even when the pointer slips off
    /// the band), the ordinary arrow everywhere else.
    fn apply_pointer_cursor(&mut self) {
        let divider_axis = self.divider_drag.as_ref().map(|drag| drag.dir).or_else(|| {
            match self.seat_pointer.hover {
                Some(seats::ChromeTarget::Divider(split)) => self
                    .seats
                    .split_slots(&self.seat_layout)
                    .into_iter()
                    .find(|slot| slot.id == split)
                    .map(|slot| slot.dir),
                _ => None,
            }
        });
        self.window
            .set_cursor(pointer_cursor(self.drag.is_some(), divider_axis));
    }

    /// Put the frame already on screen back in the slot so a pure chrome change
    /// reaches the glass. Chrome lives beside the frame, exactly as the peek
    /// flyout does, so `redraw` would otherwise find nothing queued and skip.
    fn present_chrome_change(&mut self) -> Result<()> {
        if self.pending_resize_present.is_none()
            && self.pending_frames.pending_frame().is_none()
            && let Some(frame) = self.last_presented_frame.clone()
        {
            self.pending_frames
                .publish(
                    frame,
                    FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::Expose,
                    },
                )
                .context("re-present the on-screen frame for a seat chrome change")?;
        }
        self.window.request_redraw();
        Ok(())
    }

    /// A left press on a tab's body (J105, and the first half of J99).
    ///
    /// It arms the promise and nothing else — no view changes here, which is the
    /// whole mechanism. The second press of a double click arms nothing either:
    /// the first one already put the view on this tab, so there is nothing left
    /// for it to owe.
    fn press_tab(&mut self, index: usize, position: PhysicalPosition<f64>) -> Result<()> {
        let now = Instant::now();
        let tab = self.tabs[index].id;
        // Deliberately *not* counted here. A click is complete when the button
        // comes back up, which is why `dblclick` is a release-time event;
        // counting the press as well would pair each click with itself and turn
        // the very first one into a double.
        // One button, one press: whichever source the router chose, the other is
        // not being held.
        self.pane_press = None;
        self.tab_press = Some(if index == self.active_tab {
            TabPress::settled(tab, position, now)
        } else {
            TabPress::armed(tab, position, now)
        });
        Ok(())
    }

    /// The left button coming back up over `target`.
    fn release_tab_press(
        &mut self,
        mut press: TabPress,
        target: Option<seats::ChromeTarget>,
    ) -> Result<()> {
        let over = match target {
            Some(seats::ChromeTarget::Tab(index)) => self.tabs.get(index).map(|tab| tab.id),
            _ => None,
        };
        if press.released_over(over) {
            self.activate_tab(self.tab_index(press.tab), false)?;
        }
        // The editor opens on the *second* release, which is where `dblclick`
        // fires: down, up, click, down, up, click, and only then `dblclick`
        // (mock-up 5737). The first click has already activated the tab by the
        // time this runs a second time, which is why the editor never has to
        // activate anything itself.
        let Some(clicked) = over.filter(|tab| *tab == press.tab) else {
            // Down here and up there is not a click on either, and it is not the
            // first half of one either.
            self.tab_clicks.interrupt();
            return Ok(());
        };
        if self.tab_clicks.register(clicked, Instant::now()) == TabClick::Double {
            self.open_rename(clicked)?;
        }
        Ok(())
    }

    /// The strip's live geometry — the slots every drag judgement is made
    /// against.
    fn strip_geometry(&self, now: Instant) -> seats::TabStripGeometry {
        let scale = self.renderer.metrics().scale_factor as f32;
        let (width, _) = self.renderer.presentation_geometry().swapchain_size;
        seats::tab_strip_geometry(
            width as f32,
            scale,
            &self.tab_trailers(now),
            self.active_tab,
            self.tab_scroll,
        )
    }

    /// K111 and J106 — the press has travelled 6px, so it is a drag now.
    ///
    /// The activation is not a side effect: "reordering IS commitment to the
    /// strip context: the tab in hand shows itself, whether or not the press
    /// timer had fired" (mock-up 6832-6833). Committing it here is also what
    /// pays the press's promise for good, so a drag that is later cancelled has
    /// nothing left to owe (J108).
    ///
    /// The grip is measured from the press's own origin rather than from the
    /// pointer's position now. That is what `startDrag` does (6484-6486) and it
    /// is the difference between a tab that stays where your fingers put it and
    /// one that snaps 6px sideways the instant it comes free.
    fn begin_tab_drag(&mut self, press: TabPress, position: PhysicalPosition<f64>) -> Result<()> {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == press.tab) else {
            return Ok(());
        };
        // N163's `homeWs`, read before the activation below can change the
        // answer: this is the tab whose layout the user was looking at when the
        // gesture started, and the one they mean when they aim below the strip.
        let home = self.tabs[self.active_tab].id;
        self.activate_tab(index, false)?;
        // Re-read the strip: activating may have scrolled it to reveal the tab,
        // and a grip measured against the old scroll would be wrong by exactly
        // that much.
        let index = self.tab_index(press.tab);
        let now = Instant::now();
        let Some(slot) = self.strip_geometry(now).tabs.get(index).copied() else {
            return Ok(());
        };
        self.begin_drag(
            DragSource::Tab(press.tab),
            DragCarry::Tab(TabCarry {
                grab_dx: press.latch.origin.x - f64::from(slot.body[0]),
                origin: index,
                offset: 0.0,
                moved: false,
                home,
            }),
            position,
        )
    }

    /// J118 — the press on a pane head has travelled 6px, so the pane is in the
    /// air.
    ///
    /// Nothing is measured and nothing is committed. A pane's head is not a
    /// handle the pane hangs from: the tree stays exactly as it was for the whole
    /// gesture, and the only thing that moves is the ghost. That is the mock-up's
    /// shape too — `startDrag(e, { kind: "pane", leafId })` records a leaf and
    /// nothing else (5839) — and it is why a pane drag that comes to nothing
    /// leaves no trace at all.
    ///
    /// The `.pane-close` dead zone (C35, mock-up 5837) needs no guard here: it is
    /// [`seats::ChromeTarget::PaneClose`], a target of its own, so a press on the
    /// `×` never reaches the head's arm of the router in the first place. "The
    /// button is not the bar" is true at the hit test, which is the only place it
    /// can be true once rather than everywhere.
    fn begin_pane_drag(&mut self, seat: SeatId, position: PhysicalPosition<f64>) -> Result<()> {
        // Focus mode parks the tree and refuses every pane drag ("focus mode: the
        // tree is parked, no pane drags", mock-up 5841). This build has no focus
        // mode to be in — `LayoutMode::Focus` exists in the solver and nothing in
        // `bt-app` ever constructs it — so there is no state to test and adding a
        // condition that is always false would be inventing the guard rather than
        // implementing it. It belongs with the Focus-Mode slice, which is where
        // the state it reads is born.
        self.begin_drag(DragSource::Pane(seat), DragCarry::Pane, position)
    }

    /// Everything a drag does on the way in, whatever it is carrying (J112).
    ///
    /// The three things here are the three the mock-up's `startDrag` does for
    /// every source alike: put away what was explaining the thing you just picked
    /// up, take the pointer, and record the gesture. Anything that varies by
    /// source has already happened in the caller.
    fn begin_drag(
        &mut self,
        source: DragSource,
        carry: DragCarry,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        // `hidePeek()` is the first line of `startDrag` (6482), and L135 is why:
        // a schematic left hanging under a thing that is now moving would be
        // describing where that thing used to be.
        self.hide_layout_peek()?;
        self.drag = Some(Drag {
            source,
            carry,
            pointer: position,
            landing: None,
        });
        // Hover goes quiet for the whole gesture: while something is in your hand
        // the chrome has nothing to offer the pointer, and a `×` lighting up
        // under a tab that is sliding past is an affordance that cannot be taken.
        //
        // This is also where J117's pinning lands, and deliberately not in a
        // second call of its own: clearing the hover target re-applies the
        // pointer's shape, and the shape is a function of `self.drag` — which was
        // set one line ago. One expression decides it, in one place, for both the
        // taking and the letting go.
        self.update_chrome_hover_target(None)
    }

    /// K114/K115 — hold the grabbed tab under the pointer and answer with how far
    /// it now sits from its own slot.
    ///
    /// One axis, because the strip has one: a tab dragged along it moves in `x`
    /// and the row it lives in does not move at all. Clamped to the strip's
    /// viewport, so the tab you are holding cannot be carried out over the
    /// caption buttons or off the window's left edge.
    fn track_grabbed(&self, position: PhysicalPosition<f64>) -> Option<f32> {
        let drag = self.drag?;
        let (tab, carry) = (drag.tab()?, drag.tab_carry()?);
        let index = self.tabs.iter().position(|candidate| candidate.id == tab)?;
        let geometry = self.strip_geometry(Instant::now());
        let slot = geometry.tabs.get(index)?;
        Some(grabbed_offset(
            slot.body[0],
            slot.body[2] - slot.body[0],
            geometry.viewport,
            (position.x - carry.grab_dx) as f32,
        ))
    }

    /// Where this drag would land if the hand opened now — **the seam U5 plugged
    /// into** (K123-K135).
    ///
    /// Pure: it reads the window and answers a [`DropLanding`], and the live half
    /// of whatever it answers is applied by [`Runtime::drive_drag`] afterwards.
    /// Keeping the survey and the commitment apart is what lets the geometry grow
    /// without the state machine growing with it, and it is why this takes a
    /// source and a position rather than `&self.drag`: the question "what is
    /// under the pointer" has nothing to do with how far a tab has slid.
    ///
    /// **The priority chain, and why it is in this order.**
    ///
    /// 1. **The strip, whatever is in the hand** (K123, 6786-6787). It is asked
    ///    first because the strip is a surface in its own right and sits above
    ///    the layout, not because a tab belongs to it — a *pane* over the strip
    ///    is K124's tearing, and a tab over the layout is N159's merge. Neither
    ///    source is confined to one surface, and reading the source before the
    ///    rectangle is what used to make it look like they were.
    /// 2. **A tab over its own layout is nothing** (K129, 6934). This test only
    ///    ever passes because of the flip in [`Runtime::leave_strip`]: while the
    ///    dragged tab is the one on screen there is no other layout for it to
    ///    join, and the merge would be a tab merging into itself.
    /// 3. **The layout's rim, then a pane's zones** — [`seats::aim_at_layout`],
    ///    which carries K127, K128 and K130-K134 and states the rim-before-pane
    ///    ruling at length.
    /// 4. **Never onto yourself** (K135, 7101): a pane held over its own
    ///    rectangle has no landing at all, in any zone. Applied to the aim rather
    ///    than inside it, because "which pane is this" is a fact about the
    ///    pointer and "is that pane the one in my hand" is a fact about the hand.
    ///
    /// It re-reads the strip's geometry rather than being handed it, and that is
    /// a deliberate cost: a surveyor that depends on what its caller happened to
    /// measure is a surveyor that cannot grow a branch without threading a second
    /// argument through every existing one. The price is one strip solve per
    /// pointer move, on a strip of at most a few dozen tabs.
    fn survey_drop(
        &self,
        source: DragSource,
        position: PhysicalPosition<f64>,
    ) -> Option<DropLanding> {
        let scale = self.renderer.metrics().scale_factor as f32;
        let geometry = self.strip_geometry(Instant::now());
        if seats::in_strip(&geometry, scale, position.x, position.y) {
            return self.survey_strip(source, &geometry, position);
        }
        // K129 — dragging the active tab onto its own layout is meaningless.
        if source == DragSource::Tab(self.tabs[self.active_tab].id) {
            return None;
        }
        let aim = seats::aim_at_layout(
            &self.seat_layout,
            self.layout_host_rect(),
            self.seats.pane_count(),
            scale,
            position.x,
            position.y,
        )?;
        landing_for_aim(source, aim)
    }

    /// The strip's arm of [`Runtime::survey_drop`] (K123-K125).
    ///
    /// The two sources ask the strip for different things and measure it
    /// differently, which is why they are two arms rather than one with a flag.
    /// A tab in the strip is a *body* sliding along a run and it swaps with a
    /// neighbour once it has covered half of it ([`seats::reorder_target`]); a
    /// pane arriving from the layout has no body in the strip yet, so the only
    /// operand is the pointer against the slot midpoints
    /// ([`seats::insert_index_at`], K125).
    fn survey_strip(
        &self,
        source: DragSource,
        geometry: &seats::TabStripGeometry,
        position: PhysicalPosition<f64>,
    ) -> Option<DropLanding> {
        let slot_mids = seats::tab_slot_mids(geometry);
        match source {
            DragSource::Tab(tab) => {
                let index = self.tabs.iter().position(|candidate| candidate.id == tab)?;
                let slot = geometry.tabs.get(index)?;
                let offset = self.track_grabbed(position)?;
                Some(DropLanding::StripReorder {
                    slot: seats::reorder_target(
                        &slot_mids,
                        &self.tabs.iter().map(|tab| tab.pinned).collect::<Vec<_>>(),
                        index,
                        slot_mids[index] + offset,
                        (slot.body[2] - slot.body[0]) / 2.0,
                    ),
                })
            }
            // K124 — only while both halves would be tabs. G84 is one reason and
            // it is a rule of the tree rather than of the gesture: a tree may not
            // be emptied, so the last pane has nowhere to be torn to.
            //
            // `tab_can_host` is the second, and it is the one that answers today.
            // The mock-up's guard here is `paneCount > 1` because in the mock-up
            // a pane is only a subtree; here it is a subtree *and* possibly the
            // tab's one shell, so the question is not "is anything left" but "are
            // both of these still tabs". Drawing an insertion caret in the strip
            // for a tear-out that the release cannot perform is the silent
            // refusal M147 forbids, in the one place that has no dashed box to
            // wear instead.
            DragSource::Pane(seat) => {
                self.tear_out_is_hostable(seat)
                    .then(|| DropLanding::StripExtract {
                        slot: seats::insert_index_at(&slot_mids, position.x as f32),
                    })
            }
        }
    }

    /// **N157's precondition**: whether tearing this pane out would leave two
    /// tabs this window can hold.
    ///
    /// Both halves are asked, and both have to answer, because a tear-out makes
    /// two tabs rather than moving one pane: the pane that leaves becomes a tab
    /// on its own, and what stays behind has to go on being one. `tear_out`
    /// answers `None` for the last pane in a tree (G84), so the mock-up's
    /// `paneCount > 1` is inside this question rather than beside it.
    ///
    /// It is false for every pane today and that is not a stub — it is
    /// [`tab_can_host`] reporting what the tab model is, evaluated rather than
    /// assumed. With one shell to a tab, either the pane leaving is the terminal
    /// (and the tab it leaves has none) or it is not (and the tab it makes has
    /// none). The day panes own sessions, this starts answering `true` on its own
    /// with nothing here rewritten.
    fn tear_out_is_hostable(&self, seat: SeatId) -> bool {
        self.seats
            .tear_out(&self.seat_metrics(), seat)
            .is_some_and(|(leaving, staying)| tab_can_host(&leaving) && tab_can_host(&staying))
    }

    /// The layout's own box in device pixels — what every rim distance is
    /// measured from (K128/K130).
    ///
    /// Built from the swapchain and the DPI rather than from the seats inside it,
    /// through the same helper the solver's viewport comes from, so the rim and
    /// the rectangles it competes with cannot disagree about where the layout
    /// begins.
    fn layout_host_rect(&self) -> [f64; 4] {
        let (width, height) = self.renderer.presentation_geometry().swapchain_size;
        let dpi_milli = self.renderer.metrics().dpi_milli().get();
        seats::device_viewport(width, height, seats::scale_ppm(dpi_milli))
    }

    /// Drive a drag one pointer move. Returns whether the pointer was consumed.
    ///
    /// A drag owns the pointer outright, exactly as a divider drag does: while
    /// one is in flight nothing below hears the move, so no hover lights up, no
    /// tooltip arms and no selection extends underneath it.
    ///
    /// Three steps, in the mock-up's own order (6753-6790): move the ghost, ask
    /// what is under the pointer, then let the answer do its live half. The
    /// ghost moves *first* and unconditionally, because it is the report on where
    /// the hand is and a hand that has moved has moved whether or not anything is
    /// willing to receive it.
    fn drive_drag(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(mut drag) = self.drag else {
            return Ok(false);
        };
        // What is in the hand can go away underneath the gesture — a background
        // shell exits and `reap_exited_tabs` closes its tab, or a seat is closed
        // by a verb this window ran for some other reason. There is then nothing
        // left to drag, and the state must not survive the thing it points at.
        if !self.drag_source_lives(drag.source) {
            self.drag = None;
            self.apply_pointer_cursor();
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(true);
        }
        drag.pointer = position;
        self.leave_strip(&mut drag, position)?;
        drag.landing = self.survey_drop(drag.source, position);
        if let (Some(DropLanding::StripReorder { slot }), Some(tab), Some(carry)) =
            (drag.landing, drag.tab(), drag.tab_carry())
        {
            drag.carry = DragCarry::Tab(self.settle_strip_reorder(tab, carry, slot, position));
        }
        self.drag = Some(drag);
        // The ghost lives in the overlay rather than in the chrome, and it does
        // not need its own repaint call: `refresh_chrome` rebuilds the overlay
        // from the same choke point and answers `true` if *either* changed. On a
        // pane drag the chrome is identical frame to frame and the overlay is the
        // only thing moving, which is exactly the case that choke point exists
        // for.
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Whether the thing this drag is carrying is still in the window.
    fn drag_source_lives(&self, source: DragSource) -> bool {
        match source {
            DragSource::Tab(tab) => self.tabs.iter().any(|candidate| candidate.id == tab),
            DragSource::Pane(seat) => self.seats.tree().contains(seat),
        }
    }

    /// **K126/N163 — the hand has left the strip.**
    ///
    /// Two things happen at that boundary and the mock-up does both in one place
    /// (6909-6919), because they are one sentence: the tab stops being carried
    /// along the run and the *view* goes back to where the gesture set out from.
    ///
    /// `releaseGrabbed(true)` is the first half — the tab falls into whatever
    /// slot the live reorder left it in, sliding rather than jumping, and the
    /// ghost takes over as the thing under the pointer. Nothing is committed by
    /// it: the reorder was already applied, and letting go over open air still
    /// takes it home (J120).
    ///
    /// The second half is press-activation's counterpart. Pressing a background
    /// tab activates it so you can see what you have picked up, which means the
    /// layout on screen is now the *dragged* tab's — and a tab cannot be merged
    /// into itself (K129). Aiming below the strip says "now place A somewhere in
    /// the layout I was in", so the view flips back to
    /// [`TabCarry::home`] and the merge gets a target. Without this the whole of
    /// K's lower half is unreachable for a tab: every pointer below the strip
    /// would be over the dragged tab's own layout, and K129 would answer nothing
    /// every time.
    ///
    /// Idempotent by construction, which matters because a pointer moves many
    /// times outside the strip and this runs on all of them: the slide home only
    /// has an offset to run down once, and the flip's own condition is false the
    /// moment it has happened.
    fn leave_strip(&mut self, drag: &mut Drag, position: PhysicalPosition<f64>) -> Result<()> {
        let (Some(tab), Some(mut carry)) = (drag.tab(), drag.tab_carry()) else {
            return Ok(());
        };
        let scale = self.renderer.metrics().scale_factor as f32;
        if seats::in_strip(
            &self.strip_geometry(Instant::now()),
            scale,
            position.x,
            position.y,
        ) {
            return Ok(());
        }
        if carry.offset != 0.0
            && let Some(index) = self.tabs.iter().position(|candidate| candidate.id == tab)
        {
            self.tabs[index]
                .flip
                .displace(carry.offset, Instant::now(), self.motion);
            carry.offset = 0.0;
            drag.carry = DragCarry::Tab(carry);
        }
        if carry.home != tab
            && self.tabs[self.active_tab].id == tab
            && let Some(home) = self
                .tabs
                .iter()
                .position(|candidate| candidate.id == carry.home)
        {
            self.activate_tab(home, false)?;
        }
        Ok(())
    }

    /// The live half of [`DropLanding::StripReorder`]: put the strip in the order
    /// the hand is asking for, and answer with where the tab now sits.
    ///
    /// The reorder is *applied*, not previewed. That is the mock-up's
    /// `reorderWhileDragging` (6835) and the reason it can be: the strip has one
    /// axis and one kind of occupant, so the arrangement the drop would produce
    /// is a thing the strip can simply be in while you are still deciding.
    fn settle_strip_reorder(
        &mut self,
        tab: TabId,
        mut carry: TabCarry,
        to: usize,
        position: PhysicalPosition<f64>,
    ) -> TabCarry {
        let Some(index) = self.tabs.iter().position(|candidate| candidate.id == tab) else {
            return carry;
        };
        let Some(offset) = self.track_grabbed(position) else {
            return carry;
        };
        if to == index {
            carry.offset = offset;
            return carry;
        }
        self.move_tab_with_flip(index, to, Instant::now(), Some(tab));
        carry.moved = true;
        // Its slot has moved, so the distance from the slot to the hand has
        // changed with it (mock-up 6727-6729).
        carry.offset = self.track_grabbed(position).unwrap_or(offset);
        carry
    }

    /// Move a tab between slots and let every tab the move displaced slide into
    /// its new one — K117's FLIP.
    ///
    /// The order changes first and the animation is derived from the difference,
    /// which is what makes this FLIP rather than a hand-written slide: nothing
    /// here has to know *why* the strip re-laid out, only that it did.
    ///
    /// `skip` is the tab in hand, which does not take part: it is already
    /// somewhere of its own choosing, and inverting it back to a slot it is not
    /// in would tear it out from under the pointer (K117).
    fn move_tab_with_flip(&mut self, from: usize, to: usize, now: Instant, skip: Option<TabId>) {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        let motion = self.motion;
        let active = self.tabs[self.active_tab].id;
        let before = self.slot_lefts(now);
        let was = self.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        // Everything keyed on a slot has to be re-derived from identity after the
        // order changes — the active tab most of all, because its index is what
        // the session file records.
        self.active_tab = self.tab_index(active);
        let after = self.slot_lefts(now);
        for (old_index, id) in was.into_iter().enumerate() {
            if skip == Some(id) {
                continue;
            }
            let new_index = self.tab_index(id);
            let (Some(old_left), Some(new_left)) = (before.get(old_index), after.get(new_index))
            else {
                continue;
            };
            let delta = old_left - new_left;
            if delta != 0.0 {
                self.tabs[new_index].flip.displace(delta, now, motion);
            }
        }
    }

    /// Where every slot's leading edge is, in strip order.
    fn slot_lefts(&self, now: Instant) -> Vec<f32> {
        self.strip_geometry(now)
            .tabs
            .iter()
            .map(|tab| tab.body[0])
            .collect()
    }

    /// Let go (K118, K121, J120, and the mock-up's commit table at 7202-7231).
    ///
    /// One question: what did the last survey answer?
    ///
    /// * **A landing.** It decides. The only landing this slice has is the strip
    ///   reorder, and it decided already — it was applied live, slot by slot, as
    ///   the tab travelled — so there is nothing to commit and nothing to
    ///   rebuild. The tab hands its offset to the settle, the settle runs it down
    ///   to its slot, and the strip that was already on screen carries on being
    ///   the strip (K122).
    /// * **No landing — J120.** The carried thing goes back to the slot the drag
    ///   began in, sliding rather than jumping, displaced neighbours travelling
    ///   home with it.
    ///
    /// **J120 is not a cancel and not a commit, and the distinction is not
    /// pedantry.** It shares [`Runtime::settle_home`] with Esc because the
    /// *motion* is the same one — there is only one way for a thing to go back
    /// where it came from — but nothing else about the two is. A cancel is the
    /// user retracting a gesture; this is the gesture completing and finding
    /// nowhere to be. The difference shows up in what each leaves behind: both
    /// keep the press's activation (J108), and neither writes the session,
    /// because a drag that landed nowhere chose nothing to record.
    fn release_drag(&mut self) -> Result<bool> {
        let Some(drag) = self.drag.take() else {
            return Ok(false);
        };
        let now = Instant::now();
        let motion = self.motion;
        // A gesture is not a click, and it is not half of one either.
        self.tab_press = None;
        self.pane_press = None;
        self.tab_clicks.interrupt();
        match release_verdict(drag.landing) {
            DragRelease::Commit => {
                if let (Some(tab), Some(carry)) = (drag.tab(), drag.tab_carry())
                    && let Some(index) = self.tabs.iter().position(|candidate| candidate.id == tab)
                {
                    self.tabs[index].flip.displace(carry.offset, now, motion);
                    self.tabs[index].landing.start(now, motion);
                    // The strip's order is the file's order, and a reorder is a
                    // choice the user made rather than a state being explored
                    // (§5.1). A drag that moved nothing decided nothing, and the
                    // activation it did commit has already recorded itself.
                    if carry.moved {
                        self.mark_session_dirty(now);
                    }
                }
            }
            // A drop that was refused between the last survey and the release —
            // the pointer's answer is a function of a tree and a viewport, and
            // both can move under a still hand — has landed nowhere, which is
            // exactly what J120 is for. One outcome, reached two ways.
            DragRelease::Land if self.commit_layout_drop(drag)? => {}
            DragRelease::Land | DragRelease::Home => self.settle_home(drag),
        }
        self.finish_drag()
    }

    /// **U7 — let go over the layout** (L136-L140, G81-G83, D43).
    ///
    /// The plan is computed from the drag's own inputs rather than lifted out of
    /// [`Runtime::drop_preview`], and the two are the same object for a reason
    /// that is not a coincidence: `plan_drop` is pure (T223/D2), so the same
    /// inputs give back the same tree to the bit. Reading the cache would be
    /// asking a *remembered* answer to a question the world may have changed
    /// under — the fade that keeps a retired preview alive for its hundred
    /// milliseconds is enough on its own to make the cache older than the
    /// release — and re-asking costs one tree walk on a tree of a few leaves.
    ///
    /// Everything after the adoption is what a tree change costs, in the order
    /// `close_pane` and the preview toggle already pay it: the pointer's picture
    /// is stale because the rectangles moved under it, the window's own minimum
    /// is a function of the tree, and the terminal's columns are a function of
    /// its seat's rectangle. `commit_seat_geometry` marks the session dirty on
    /// the way through, so the strip's order, the tree's shape and the focused
    /// leaf all reach disk through the one channel that already carries them.
    ///
    /// Answers whether the tree changed.
    fn commit_layout_drop(&mut self, drag: Drag) -> Result<bool> {
        let Some(inputs) = self.plan_inputs_for(drag) else {
            return Ok(false);
        };
        let Some(plan) = self.plan_for(&inputs) else {
            return Ok(false);
        };
        if self.seats.adopt_drop(plan).is_none() {
            return Ok(false);
        }
        self.seat_pointer = seats::ChromePointer::default();
        self.divider_drag = None;
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()?;
        Ok(true)
    }

    /// J119 — "never mind".
    ///
    /// Esc, and the pointer stream ending without a button-up. Everything the
    /// gesture put on screen comes down and **no drop is committed**; the carried
    /// thing goes home by the same route J120 uses.
    ///
    /// **Deviation, recorded.** The mock-up's `cancelDrag` settles the tab into
    /// whatever slot the live reorder last put it in (7153-7165) — it undoes the
    /// *drop*, not the reordering. J120 rules that the native build reads the
    /// mock-up's own sentence literally instead: the reorder is as much a commit
    /// as the drop is, it was made by the same gesture, and a cancel that keeps
    /// half of what it cancelled leaves the user to undo the rest by hand.
    ///
    /// What Esc does *not* undo is the activation (J108): the press chose this
    /// tab, and a cancelled drag does not unchoose it. Nothing here has to say so
    /// — the promise was paid the moment the drag began.
    fn cancel_drag(&mut self) -> Result<bool> {
        let Some(drag) = self.drag.take() else {
            return Ok(false);
        };
        self.tab_press = None;
        self.pane_press = None;
        self.tab_clicks.interrupt();
        self.settle_home(drag);
        self.finish_drag()
    }

    /// Put the carried thing back in the slot the drag began in — the FLIP home
    /// J119 and J120 share.
    ///
    /// A pane has no slot to return to and no offset to run down: it never left.
    /// The tree was untouched for the whole gesture (see
    /// [`Runtime::begin_pane_drag`]), so "back where it started" is where it
    /// already is, and the honest implementation of going home is to do nothing.
    /// That is not a gap — it is the reason a pane drag is safe to abandon at any
    /// moment.
    fn settle_home(&mut self, drag: Drag) {
        let (Some(tab), Some(carry)) = (drag.tab(), drag.tab_carry()) else {
            return;
        };
        let Some(index) = self.tabs.iter().position(|candidate| candidate.id == tab) else {
            return;
        };
        let now = Instant::now();
        let motion = self.motion;
        // Hand the settle the offset first, so the slide home starts where the
        // hand left the tab and the slot change below composes onto it.
        self.tabs[index].flip.displace(carry.offset, now, motion);
        let pinned = self.tabs.iter().map(|tab| tab.pinned).collect::<Vec<_>>();
        // F57 again, and it has to be re-applied rather than trusted: between the
        // drag starting and it ending, a pinned tab may have been reaped, which
        // shifts every index after it by one.
        let to = partition_clamped(&pinned, index, carry.origin.min(pinned.len() - 1));
        self.move_tab_with_flip(index, to, now, None);
    }

    /// What both exits do once the gesture's own business is finished.
    ///
    /// One place, because these are the four things that are true of *ending* a
    /// drag rather than of any particular way of ending one — and the mock-up
    /// puts the same four in both `cancelDrag` and its `pointerup` (7166-7183
    /// against 7189-7201) for exactly that reason.
    /// Answers `true` — a drag that got this far consumed the event that ended
    /// it, which is what both callers report to their own callers.
    fn finish_drag(&mut self) -> Result<bool> {
        // J117 in reverse: the pointer stops being pinned the instant the hand
        // is empty, and takes back the shape of whatever it is now over.
        self.apply_pointer_cursor();
        // Hover was frozen at "nothing" for the whole gesture; the pointer has
        // not moved, but what is under it has.
        if let Some(position) = self.pointer_position {
            self.update_chrome_hover(position)?;
        }
        // Taking the ghost down is an overlay change, and on the frame a pane
        // drag ends it is the *only* change there is — which `refresh_chrome`
        // reports, because it owns the overlay's rebuild too.
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Where a tab is now, by identity.
    fn tab_index(&self, tab: TabId) -> usize {
        self.tabs
            .iter()
            .position(|candidate| candidate.id == tab)
            .unwrap_or(self.active_tab)
    }

    /// Open the tab-name editor (J99-J101, mock-up 5854-5870).
    fn open_rename(&mut self, tab: TabId) -> Result<()> {
        let Some(index) = self.tabs.iter().position(|candidate| candidate.id == tab) else {
            return Ok(());
        };
        // Mock-up 5858-5859: a tab with no session to name does not open an
        // editor. Every tab in this build has one, so this is the stub J104 asks
        // for — the guard exists and is asked, and T5's files-only tab will be
        // the first thing it turns away.
        if !self.tabs[index].seed().can_be_named() {
            return Ok(());
        }
        self.rename = Some(TabRename::open(
            tab,
            self.tabs[index].manual_name.as_deref(),
        ));
        // A caret that arrives mid-blink arrives invisible half the time.
        self.rename_blink.reset(Instant::now());
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Close the editor, writing the draft through or throwing it away.
    ///
    /// "Escape restores and leaves; Enter and blur commit. Two paths, not
    /// three" (mock-up 5847-5849). Doing nothing when no editor is open is the
    /// point rather than an oversight: this is called from every blur-shaped
    /// event in the window, and most of the time there is nothing to blur.
    fn finish_rename(&mut self, commit: bool) -> Result<()> {
        let Some(editor) = self.rename.take() else {
            return Ok(());
        };
        if commit && let Some(index) = self.tabs.iter().position(|tab| tab.id == editor.tab) {
            let name = editor.committed_name();
            if self.tabs[index].manual_name != name {
                self.tabs[index].manual_name = name;
                // The seed reads `manual_name` (`TabState::term_leaf`), so the
                // vault, the session file and the restore prompt all pick the
                // new name up from here without a second write — and the OS
                // window title is the active tab's own.
                if index == self.active_tab {
                    self.window.set_title(&self.display_title());
                }
                self.mark_session_dirty(Instant::now());
            }
        }
        // Unconditional, exactly as the mock-up's `finish` is (5885-5889): the
        // commonest exit is opening the editor, changing your mind and clicking
        // away, where the state is byte-identical to before and only the drawing
        // is wrong.
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Route an event-loop tick to the press promise and the rename caret.
    fn advance_tab_press_if_due(&mut self, now: Instant) -> Result<()> {
        let matured = self
            .tab_press
            .as_mut()
            .is_some_and(|press| press.matured(now));
        if !matured {
            return Ok(());
        }
        let tab = self.tab_press.expect("a press that matured is a press").tab;
        self.activate_tab(self.tab_index(tab), false)
    }

    fn advance_rename_blink_if_due(&mut self, now: Instant) -> Result<()> {
        if self.rename.is_none() || !self.rename_blink.advance(now) {
            return Ok(());
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Route a press onto seat chrome. Returns whether the button was consumed.
    fn chrome_mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        if button == MouseButton::Middle {
            if state == ElementState::Pressed
                && let Some(seats::ChromeTarget::Tab(index)) = self.chrome_target_at(position)
            {
                // Closing a tab with the wheel is not the first half of anything.
                self.tab_clicks.interrupt();
                self.close_tab(index)?;
                return Ok(true);
            }
            return Ok(matches!(
                self.chrome_target_at(position),
                Some(seats::ChromeTarget::Tab(_))
            ));
        }
        if button != MouseButton::Left {
            return Ok(false);
        }
        if state == ElementState::Released {
            // Ahead of the press: a gesture that has become a drag answers with
            // its drop, and the press that started it is no longer a click.
            if self.release_drag()? {
                return Ok(true);
            }
            if self.divider_drag.take().is_some() {
                self.seat_pointer.dragging = None;
                self.apply_pointer_cursor();
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                // The end of a drag is a meaningful change (§5.1): the ratio that
                // was being explored is now the ratio the user chose.
                self.mark_session_dirty(Instant::now());
                return Ok(true);
            }
            let target = self.chrome_target_at(position);
            // A pane press that never travelled has nothing to settle: D40 moved
            // the focus on the way down and that is all a press on a head has
            // ever meant. Dropping it is the whole of letting go.
            let held_pane = self.pane_press.take().is_some();
            if let Some(press) = self.tab_press.take() {
                self.release_tab_press(press, target)?;
                return Ok(true);
            }
            if held_pane {
                return Ok(true);
            }
            return Ok(target.is_some());
        }
        let target = self.chrome_target_at(position);
        // D40, above the router and consuming nothing: every press inside a pane
        // moves the layout focus there, whatever else the press goes on to mean.
        // Above the rename guard too — clicking into another pane is a blur, and
        // a blur that committed the editor still landed in that pane.
        self.focus_pane_at(position)?;
        // Blur commits, and blur is every press that is not inside the editor
        // (J102; mock-up 5898 `input.addEventListener("blur", () => finish(true))`,
        // which the browser fires on `pointerdown`, before the press does
        // anything else — so this guard stands above the whole router for the
        // same reason).
        //
        // **Ruling.** The editor's own extent is the *whole tab body*, not a
        // sub-box inside it. The mock-up stops propagation on the `<input>`
        // (5899) and the input is only part of the tab; but its central claim is
        // that "the editor is the tab" (376-378), and honouring that means the
        // tab's padding and its mark belong to the editor too. The alternative
        // is a strip of pixels inside the tab you are typing in where a click
        // silently commits, which is the kind of edge nobody discovers on
        // purpose. The `×` and the pin stay buttons — they are the two things in
        // the tab that were never the title.
        if self.rename.is_some() {
            let editing = self
                .rename
                .as_ref()
                .and_then(|editor| self.tabs.iter().position(|tab| tab.id == editor.tab));
            if target == editing.map(seats::ChromeTarget::Tab) {
                // "编辑器内的按下/双击不触发拖拽或再次进入编辑" (J103): the press
                // is consumed whole — no promise armed, no click recorded.
                self.tab_clicks.interrupt();
                return Ok(true);
            }
            self.finish_rename(true)?;
        }
        let Some(target) = target else {
            // Not on chrome, but possibly not on the terminal either — a press
            // in a preview's body belongs to that seat and must not reach the
            // grid underneath it. With a lone leaf there is no other seat for a
            // press to belong to, so nothing is claimed and every existing path
            // sees the button exactly as before.
            self.tab_clicks.interrupt();
            return Ok(!self.seats.is_lone_terminal()
                && !seats::terminal_contains(
                    &self.seat_layout,
                    self.seats.terminal(),
                    position.x,
                    position.y,
                ));
        };
        match target {
            seats::ChromeTarget::Divider(split) => {
                let Some(slot) = self
                    .seats
                    .split_slots(&self.seat_layout)
                    .into_iter()
                    .find(|slot| slot.id == split)
                else {
                    return Ok(true);
                };
                let Some(origin) = self
                    .seats
                    .tree()
                    .ratios()
                    .into_iter()
                    .find_map(|(id, ratio)| (id == split).then_some(ratio))
                else {
                    return Ok(true);
                };
                self.divider_drag = Some(DividerDrag {
                    split,
                    dir: slot.dir,
                    origin,
                });
                self.seat_pointer.dragging = Some(split);
                self.apply_pointer_cursor();
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
            }
            seats::ChromeTarget::CollapseBar(seat) => {
                // §2.6.3: clicking a collapsed bar expands it, by promoting it
                // to the focus — W2 then makes it the last seat to fall, and
                // the concession chain gives it the room by itself. Keyboard
                // focus does not move; v1 keeps that on the terminal.
                //
                // §2.6.3 asks for three ways in and this is one of them. The
                // other two are recorded rather than approximated:
                //
                // * *Tab-reachable, Enter to expand.* There is no chrome focus
                //   ring in this build at all — `Tab` is forwarded to the PTY as
                //   a byte (`input.rs`), which is the correct behaviour while the
                //   terminal owns the keyboard. A ring is a window-wide decision
                //   about who owns `Tab` and in what order, not a thing one bar
                //   may invent for itself; it belongs with D45/O173, the
                //   still-open ruling on whether this product has *any* keyboard
                //   route between panes.
                // * *Selectable from the command palette.* There is no command
                //   palette. `Ctrl+Shift+P` is deliberately kept clear for it
                //   (see the dev-only chord below), and O166 already records that
                //   the palette has no pane entries of any kind to be consistent
                //   with.
                //
                // Both are keyboard reach, and a bar that can only be clicked is
                // exactly as reachable as every other pane in this build — which
                // is the honest statement of where the gap is: it is not the
                // collapsed bar's, it is the whole block's.
                if self.seats.set_focus(seat) {
                    self.apply_window_min_inner_size()?;
                    self.commit_seat_geometry()?;
                }
            }
            // D40's focus move is done for every press in the pane by
            // `focus_pane_at` above the router. What the head adds is J118: it is
            // the pane's handle, so the press arms the six pixels and waits.
            //
            // Nothing is consumed and nothing is shown. A press on a head that
            // never travels is a press that meant "put me in this pane", which
            // has already happened — so arming costs the click nothing, and that
            // is exactly the mock-up's shape (`pointerdown` → `startDrag`, with
            // the ordinary click handler left to run, 5835-5840).
            seats::ChromeTarget::PaneHeader(seat) => {
                self.tab_press = None;
                self.pane_press = Some(PanePress {
                    seat,
                    latch: DragLatch::new(position),
                });
            }
            // I102/I105: one verb for every kind of leaf. A files pane has no
            // session to clear, which is the whole of what `closeFilesPane =
            // closePane` means (mock-up 3579).
            seats::ChromeTarget::PaneClose(seat) => {
                self.tab_clicks.interrupt();
                self.close_pane(seat)?;
            }
            seats::ChromeTarget::Tab(index) => self.press_tab(index, position)?,
            // J99: "`.close`/`.pin` 上的双击不算(那是两次按钮点击)". Neither
            // records a click, so neither can be half of a rename — and both
            // break a chain that was already running.
            seats::ChromeTarget::TabClose(index) => {
                self.tab_clicks.interrupt();
                self.close_tab(index)?;
            }
            // F61 — the pin stands in the `×`'s slot, so unpinning is exactly
            // where you already are.
            seats::ChromeTarget::TabPin(index) => {
                self.tab_clicks.interrupt();
                self.toggle_pin(index)?;
            }
            seats::ChromeTarget::NewTab => {
                self.tab_clicks.interrupt();
                self.new_tab()?;
            }
            seats::ChromeTarget::NewTabMenu => self.toggle_profile_menu()?,
            seats::ChromeTarget::Settings => self.toggle_settings_panel()?,
            seats::ChromeTarget::Minimize => self.window.set_minimized(true),
            seats::ChromeTarget::Maximize => {
                self.window.set_maximized(!self.window.is_maximized());
            }
            seats::ChromeTarget::CloseWindow => {
                let hwnd = window_hwnd(&self.window)?;
                bt_platform::request_window_close(hwnd)
                    .map_err(|error| anyhow!(error))
                    .context("request self-drawn caption close")?;
            }
        }
        Ok(true)
    }

    fn mouse_input(&mut self, state: ElementState, button: MouseButton) -> Result<()> {
        // M142, and ahead of everything: any press at all takes the tip down.
        // Unconditional — not "a press that hits something", not "a left press" —
        // because a tooltip answers "what is this?" and the act of pressing is
        // you saying you already know. The mock-up's listener is the document's
        // for the same reason.
        if state == ElementState::Pressed {
            self.hide_tooltip()?;
            // L135 sends the peek the same way and for the same reason: it is a
            // glance, and pressing is you saying you are done glancing.
            self.hide_layout_peek()?;
        }
        // A modal means MODAL. Ahead of the chrome router, so the caption run —
        // the gear included — is behind the scrim like everything else, and no
        // press reaches a divider, a seat, the terminal's selection or a peek.
        if let (Some(layout), Some(position)) = (self.settings_layout(), self.pointer_position) {
            return self.settings_mouse_input(&layout, state, button, position);
        }
        // The prompt takes the press only where it is drawn — it is a prompt over
        // a working app, not a gate in front of one, so a press anywhere else is
        // still the press it always was and reaches the terminal underneath.
        if let (Some(position), Some(layout)) = (self.pointer_position, self.restore_layout())
            && let Some(target) = restore::hit(&layout, position.x, position.y)
        {
            if state == ElementState::Pressed
                && button == MouseButton::Left
                && let Some(answer) = restore::answer(target)
            {
                self.answer_restore_prompt(answer)?;
            }
            return Ok(());
        }
        // The picker takes the press only where it is drawn. A press on a row
        // starts that profile's tab; a press on the menu's own padding is the
        // menu's and does nothing; a press anywhere else puts it away and then
        // goes on to be the press it always was.
        if let (Some(layout), Some(position)) = (self.profile_menu_layout(), self.pointer_position)
        {
            match profiles::hit(&layout, position.x, position.y) {
                Some(row) => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        self.close_profile_menu()?;
                        // The row says which door it is. An untagged index here
                        // would have started PowerShell every time somebody
                        // clicked a Recent entry — the same row number means two
                        // different things in two different sections.
                        match row {
                            Some(profiles::MenuRow::Profile(index)) => {
                                self.new_tab_with_profile(index)?;
                            }
                            Some(profiles::MenuRow::Recent(index)) => {
                                self.reopen_recent(index)?;
                            }
                            None => {}
                        }
                    }
                    return Ok(());
                }
                None => {
                    if state == ElementState::Pressed
                        && !matches!(
                            self.chrome_target_at(position),
                            Some(seats::ChromeTarget::NewTabMenu)
                        )
                    {
                        self.close_profile_menu()?;
                    }
                }
            }
        }
        if let Some(position) = self.pointer_position
            && self.chrome_mouse_input(state, button, position)?
        {
            return Ok(());
        }
        if state == ElementState::Released
            && matches!(self.mouse_route, Some(MouseRoute::MathBlock))
        {
            self.mouse_route = None;
            return Ok(());
        }
        if state == ElementState::Pressed
            && let Some(math_hit) = self.math_hit()
            && matches!(button, MouseButton::Left | MouseButton::Right)
        {
            // Formula pixels are one indivisible presentation object in this slice. Swallowing the
            // complete press/release pair intentionally prevents half-source selections and keeps
            // both local selection and application mouse reporting from seeing synthetic cells.
            self.mouse_route = Some(MouseRoute::MathBlock);
            match (button, math_hit.target) {
                (MouseButton::Left, MathHitTarget::ToggleSource) => {
                    if self.session.toggle_math_source(&math_hit.anchor) {
                        self.clear_selection();
                        self.publish_interaction_frame()?;
                    }
                }
                (MouseButton::Left, MathHitTarget::CopyLatex) => {
                    self.copy_math_latex(&math_hit.anchor);
                }
                (MouseButton::Right, _) => match self.math_context_menu.request() {
                    Ok(true) => {
                        self.pending_math_context_anchor = Some(math_hit.anchor.clone());
                    }
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!("recoverable formula context-menu queue failure: {error}");
                    }
                },
                (MouseButton::Left, MathHitTarget::Block) => {
                    if self.session.view_selection().is_some() {
                        self.clear_selection();
                        self.publish_interaction_frame()?;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        let Some(hit) = self.frame_hit() else {
            return Ok(());
        };
        let Some(protocol_button) = protocol_mouse_button(button) else {
            return Ok(());
        };
        let modes = self.session.terminal_modes();
        let frame = self
            .last_presented_frame
            .as_ref()
            .context("missing frame for forwarded mouse hit")?;
        let forwarded_hit = live_viewport_mouse_hit(frame, hit);
        if let Some(bytes) = route_forwarded_mouse_button(
            &mut self.mouse_route,
            state,
            protocol_button,
            forwarded_hit,
            modes,
            self.modifiers,
        ) {
            return self.send_user_input(
                &bytes,
                "forward mouse button event to PTY",
                UserInputKind::Mouse,
            );
        }
        match state {
            ElementState::Pressed if button == MouseButton::Left => self.begin_local_selection(hit),
            ElementState::Released => {
                self.extend_local_selection(hit)?;
                let release_hyperlink = self.hyperlink_hit(hit);
                let release_local_image_path = self.local_image_path_hit(hit);
                let (single_click, hyperlink_to_open, local_image_action) =
                    if let Some(MouseRoute::Local(SelectionDrag {
                        mode: SelectionDragMode::Linear,
                        origin_row,
                        origin_column,
                        hyperlink,
                        open_hyperlink_on_release,
                        local_image_activation,
                        ..
                    })) = self.mouse_route.as_ref()
                        && (*origin_row, *origin_column) == (hit.row, hit.column)
                    {
                        (
                            true,
                            open_hyperlink_on_release
                                .then(|| hyperlink.clone())
                                .flatten()
                                .filter(|pressed| release_hyperlink.as_ref() == Some(pressed)),
                            local_image_activation
                                .path()
                                .is_some_and(|pressed| {
                                    release_local_image_path.as_deref() == Some(pressed)
                                })
                                .then(|| local_image_activation.clone()),
                        )
                    } else {
                        (false, None, None)
                    };
                let copy_on_select =
                    should_copy_on_select_release(self.mouse_route.as_ref(), single_click);
                self.mouse_route = None;
                if single_click {
                    self.clear_selection();
                    self.publish_interaction_frame()?;
                } else if copy_on_select {
                    self.copy_selection_on_release();
                }
                if let Some(hyperlink) = hyperlink_to_open {
                    self.activate_hyperlink(hyperlink)?;
                }
                if let Some(activation) = local_image_action {
                    match activation {
                        LocalImageActivation::None => {}
                        LocalImageActivation::Preview(path) => self.open_preview_image(path)?,
                        LocalImageActivation::External(path) => {
                            self.activate_local_image_path(&path)
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// A wheel notch over the tab strip, turned into horizontal motion (A7/A8).
    fn scroll_tab_strip(&mut self, delta: MouseScrollDelta) -> Result<()> {
        let scale = self.renderer.metrics().scale_factor as f32;
        let width = self.renderer.presentation_geometry().swapchain_size.0 as f32;
        let geometry = seats::tab_strip_geometry(
            width,
            scale,
            &self.tab_trailers(Instant::now()),
            self.active_tab,
            self.tab_scroll,
        );
        let travel = match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                // A strip has no lines of its own to count, so a notch moves one
                // wheel-amount of *this product's* line — the same distance the
                // terminal under it would have moved. A notch that changed length
                // depending on what it was over is a distance the hand has to
                // relearn at every surface.
                let line = self.projection.cell_height_subpixels().get() as f32
                    / bt_viewport::SUBPIXELS_PER_PX as f32;
                let amount =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f32 * line,
                        // A page of a horizontal scroller is a screenful of it.
                        bt_platform::WheelScrollAmount::Page => geometry.viewport[1],
                    };
                // A horizontal wheel says what it means. A vertical one over a
                // scroller that only has a horizontal axis is the case that has
                // to be translated, and translating it is why a one-axis mouse
                // can reach the far end of the strip at all.
                if x != 0.0 { x * amount } else { y * amount }
            }
            MouseScrollDelta::PixelDelta(position) => {
                // A trackpad gesture already speaks pixels, and it has both axes:
                // honour whichever one the fingers actually moved along.
                let (x, y) = (position.x as f32, position.y as f32);
                if x.abs() >= y.abs() { x } else { y }
            }
        };
        // Wheel-up reveals what lies to the left, which is a smaller offset.
        let scrolled = (self.tab_scroll - travel).clamp(0.0, geometry.max_scroll);
        if scrolled == self.tab_scroll {
            return Ok(());
        }
        self.tab_scroll = scrolled;
        // The strip moved under a stationary pointer, so what it is over changed
        // without the pointer having done anything.
        if let Some(position) = self.pointer_position {
            self.seat_pointer.hover = self.chrome_target_at(position);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    fn mouse_wheel(&mut self, delta: MouseScrollDelta) -> Result<()> {
        // A notch behind the scrim is nobody's. The dialog's own content fits,
        // so there is nothing here for a wheel to move — and scrolling the
        // terminal under a modal is the same violation as clicking it.
        if self.settings_layout().is_some() {
            return Ok(());
        }
        // A7/A8 — a notch over the tab strip is the strip's. The mock-up gives
        // `.tabs-inline` `overflow-x: auto`, and a wheel over an overflowing
        // scroller scrolls it; a vertical wheel over a horizontal-only scroller
        // is exactly the case browsers translate into horizontal motion, because
        // most mice have no second axis to offer.
        if let Some(position) = self.pointer_position
            && seats::tab_strip_contains(
                self.renderer.presentation_geometry().swapchain_size.0 as f32,
                self.renderer.metrics().scale_factor as f32,
                self.tabs.len(),
                position.x,
                position.y,
            )
        {
            return self.scroll_tab_strip(delta);
        }
        // A notch belongs to the pane it is over. With one terminal that is the
        // terminal or nothing, which is what this guard has always said; with a
        // fleet it is whichever terminal pane the pointer is in, and a notch
        // over a files column or a preview is still nobody's.
        //
        // Routing by the pointer rather than by focus is what the rest of the
        // desktop does, and it is the only reading that lets you read a build
        // log in one pane while typing in the other — which is the reason to
        // have two panes at all.
        let target_seat = match self.pointer_position {
            Some(position) => match seats::pane_at(&self.seat_layout, position.x, position.y) {
                Some(seat) if self.sessions.contains_key(&seat) => seat,
                // Over a pane that is not a terminal: nobody's notch.
                Some(_) => return Ok(()),
                // Off every pane — before the pointer has ever moved, a lone
                // leaf still scrolls exactly as it always has.
                None if self.seats.is_lone_terminal() => self.focused_leaf,
                None => return Ok(()),
            },
            None => self.focused_leaf,
        };
        // Scrolling moves the content the flyout was anchored to; the transient peek dissolves.
        self.dismiss_peek()?;
        // One physical event, two currencies. Local routes scroll by exact subpixels (stage C of
        // the pixel-scroll plan); forwarding routes speak whole wheel lines because that is the
        // application protocol. Route is decided first, then only that route's accumulator moves.
        let cell_subpixels = self.projection.cell_height_subpixels().get() as f64;
        let event_subpixels = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                let multiplier =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f64,
                        bt_platform::WheelScrollAmount::Page => self.grid.rows.get() as f64,
                    };
                f64::from(y) * multiplier * cell_subpixels
            }
            MouseScrollDelta::PixelDelta(position) => {
                position.y * bt_viewport::SUBPIXELS_PER_PX as f64
            }
        };
        if let Some(math_hit) = self.math_hit() {
            // A hovered math block pans by whole pixels derived from the same exact motion, so
            // trackpads feel identical over blocks and text. The commit is tentative: nothing is
            // taken from the local accumulator until the block actually scrolls, because a
            // non-scrollable block falls through to the ordinary routes with the event intact.
            let mut tentative = self.local_wheel_subpixel_remainder + event_subpixels;
            let take_px = drain_whole_units(&mut tentative, bt_viewport::SUBPIXELS_PER_PX as f64);
            let delta_px = i32::try_from(-take_px).unwrap_or(0);
            if delta_px == 0 {
                self.local_wheel_subpixel_remainder = tentative;
                return Ok(());
            }
            let horizontal = if self.modifiers.shift_key() {
                delta_px
            } else {
                0
            };
            let vertical = if self.modifiers.shift_key() {
                0
            } else {
                delta_px
            };
            // The anchor came from the pane under the pointer, so the block that
            // pans has to be that pane's too — asking the focused session to
            // scroll another pane's block would find no such block, or worse,
            // one that happens to share an anchor.
            let active = self.active_tab;
            if self.tabs[active]
                .sessions
                .get_mut(&target_seat)
                .is_some_and(|leaf| {
                    leaf.session
                        .scroll_math_block(&math_hit.anchor, horizontal, vertical)
                })
            {
                self.local_wheel_subpixel_remainder = tentative;
                return self.publish_interaction_frame();
            }
        }
        let target_is_focused = target_seat == self.focused_leaf;
        let modes = self.sessions.get(&target_seat).map_or_else(
            || self.session.terminal_modes(),
            |leaf| leaf.session.terminal_modes(),
        );
        // Sticky local review: while the alternate-screen viewport is displaced into the
        // projection-local overflow (Shift+wheel entered it), the user is looking at displaced
        // pixels, not the application's live pane — forwarding wheel bytes there would scroll a
        // surface the user cannot see. Plain wheel therefore stays local in both directions;
        // scrolling back to the resting bottom (offset 0) exits and restores forwarding.
        let target_is_scrolled = self
            .sessions
            .get(&target_seat)
            .is_some_and(|leaf| leaf.projection.is_scrolled());
        if modes.alternate_screen && target_is_scrolled {
            return self.scroll_view_exact_in(target_seat, event_subpixels);
        }
        // Wheel *bytes* only ever reach the pane that has the keyboard. A mouse
        // report carries a row and a column, and those coordinates are only
        // meaningful to the shell whose grid they were measured in; sending an
        // unfocused pane's notch to the focused pane's TUI would move a cursor
        // in a program the pointer is nowhere near. An unfocused TUI simply
        // waits to be clicked into — which is also how it gets a keystroke.
        if target_is_focused
            && !self.modifiers.shift_key()
            && modes.mouse_tracking != MouseTracking::Off
        {
            // Mouse-protocol wheel reports are per-notch, never per-system-scroll-line: the
            // application applies its own lines-per-event step, so multiplying by the Windows
            // wheel setting had TUIs (user report 2026-08-01: Claude Code transcript) scrolling
            // three times too far per notch.
            let notches = self.take_forward_wheel_notches(delta);
            if notches == 0 {
                return Ok(());
            }
            let Some(hit) = self.frame_hit() else {
                return Ok(());
            };
            let frame = self
                .last_presented_frame
                .as_ref()
                .context("missing frame for forwarded wheel hit")?;
            let hit = live_viewport_mouse_hit(frame, hit);
            let button = if notches > 0 {
                input::MouseProtocolButton::WheelUp
            } else {
                input::MouseProtocolButton::WheelDown
            };
            let one = input::mouse_bytes(
                modes.sgr_mouse,
                button,
                input::MouseProtocolEvent::Press,
                hit.row,
                hit.column,
                self.modifiers,
            );
            return self.send_user_input(
                &one.repeat(notches.unsigned_abs() as usize),
                "forward SGR mouse wheel to PTY",
                UserInputKind::Mouse,
            );
        }
        if modes.alternate_screen {
            // Alternate-screen wheel emulation belongs to the application. Shift is the explicit
            // local override for reviewing projection-only rows displaced above this screen.
            if self.modifiers.shift_key() {
                return self.scroll_view_exact_in(target_seat, event_subpixels);
            }
            // Arrow-key emulation is bytes, so it obeys the same rule the SGR
            // route above does: only the focused pane's shell is written to.
            if modes.alternate_scroll && target_is_focused {
                let lines = self.take_forward_wheel_lines(delta);
                if lines == 0 {
                    return Ok(());
                }
                let bytes =
                    input::alternate_scroll_bytes(lines, self.session.application_cursor_mode());
                return self.send_user_input(
                    &bytes,
                    "forward alternate-screen wheel to PTY",
                    UserInputKind::Mouse,
                );
            }
            return Ok(());
        }
        self.scroll_view_exact_in(target_seat, event_subpixels)
    }

    /// Whole-line quantization for the alternate-scroll emulation route: arrow-key emulation
    /// mirrors the local scroll distance, so the system lines-per-notch setting applies here.
    /// Fractional motion parks in the forwarding accumulators, never the local subpixel one.
    fn take_forward_wheel_lines(&mut self, delta: MouseScrollDelta) -> i32 {
        match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                let multiplier =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f64,
                        bt_platform::WheelScrollAmount::Page => self.grid.rows.get() as f64,
                    };
                self.line_wheel_remainder += f64::from(y) * multiplier;
                drain_whole_units(&mut self.line_wheel_remainder, 1.0) as i32
            }
            MouseScrollDelta::PixelDelta(position) => {
                self.pixel_wheel_remainder += position.y;
                let cell_px = self.renderer.metrics().cell_height_px as f64;
                drain_whole_units(&mut self.pixel_wheel_remainder, cell_px) as i32
            }
        }
    }

    /// Per-notch quantization for the mouse-protocol route: one wheel report per detent, the
    /// xterm convention every TUI calibrates its own scroll step against. Trackpad pixel deltas
    /// emit one report per accrued cell height of travel.
    fn take_forward_wheel_notches(&mut self, delta: MouseScrollDelta) -> i32 {
        match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                self.notch_wheel_remainder += f64::from(y);
                drain_whole_units(&mut self.notch_wheel_remainder, 1.0) as i32
            }
            MouseScrollDelta::PixelDelta(position) => {
                self.pixel_wheel_remainder += position.y;
                let cell_px = self.renderer.metrics().cell_height_px as f64;
                drain_whole_units(&mut self.pixel_wheel_remainder, cell_px) as i32
            }
        }
    }

    /// Local pixel-exact wheel consumption: accumulate the event's fractional subpixels and
    /// scroll by the integral part. Positive subpixels move into history, matching the wheel's
    /// upward direction, and residue below one subpixel simply waits for the next event.
    /// Scroll one pane's view by an exact subpixel amount.
    ///
    /// The seat is named rather than assumed because the wheel belongs to the
    /// pane under the pointer, which is not always the pane under the keyboard.
    /// The remainder accumulator stays window-wide: it holds the fraction of a
    /// subpixel one physical notch left over, and a notch is a property of the
    /// mouse, not of the pane it landed on.
    fn scroll_view_exact_in(
        &mut self,
        seat: bt_layout::SeatId,
        event_subpixels: f64,
    ) -> Result<()> {
        self.local_wheel_subpixel_remainder += event_subpixels;
        let take = drain_whole_units(&mut self.local_wheel_subpixel_remainder, 1.0);
        if take == 0 {
            return Ok(());
        }
        let active = self.active_tab;
        let Some(leaf) = self.tabs[active].sessions.get_mut(&seat) else {
            return Ok(());
        };
        leaf.projection.scroll_by_subpixels(take);
        self.publish_interaction_frame()
    }

    fn keyboard_input(&mut self, event: &KeyEvent) -> Result<()> {
        if event.state != ElementState::Pressed {
            return Ok(());
        }
        let now = Instant::now();
        if self.reset_cursor_blink(now) {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Keyboard,
            })?;
        }
        // Any keystroke dismisses the transient peek flyout (Esc included, per the peek verb
        // ruling) without consuming the key: typing means the user has moved on from hovering.
        self.dismiss_peek()?;

        // Esc unwinds one layer per press, top-most first, and a drag in flight
        // is the top-most layer there is: "Esc mid-drag means 'never mind', and
        // without this the drop still committed on pointerup" (mock-up 6045-6051).
        // It stands above even the modal, because a drag is a gesture the user is
        // in the middle of making and a dialog is one they have already opened.
        //
        // A resize is a drag for this purpose too, and it stands beside the tab
        // drag rather than below it: J122 rules the two mutually exclusive — one
        // gesture owns the pointer at a time — so the order between them is a
        // formality and only one of the two calls can ever answer `true`.
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && (self.cancel_drag()? || self.cancel_divider_drag()?)
        {
            return Ok(());
        }
        // A modal owns the keyboard. Esc unwinds one layer per press (§7.1.5:
        // the open menu first, then the dialog); every other key is swallowed
        // rather than typed into a terminal the user cannot see. This sits above
        // the IME branch on purpose — a composition that outlived the dialog
        // opening must not be able to reach the child either.
        if self.settings_layout().is_some() {
            if matches!(event.logical_key, Key::Named(NamedKey::Escape))
                && !event.repeat
                && self.settings.close_one_layer()
            {
                if let Some(position) = self.pointer_position
                    && !self.settings.is_open()
                {
                    self.update_chrome_hover(position)?;
                }
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
            }
            return Ok(());
        }
        // The tab-name editor owns the keyboard while it is open (J103;
        // `docs/DESIGN.md` §7.1.5 `InputOwner = Rename`). It sits directly under
        // the modal for the same reason the modal sits where it does — the thing
        // underneath is a terminal, and every key that escapes this branch is a
        // key typed into a shell the user is not looking at. Escape is consumed
        // here rather than falling through to §7.1.5's PTY pass-through, which
        // is exactly what that layering says: Esc reaches the child only when
        // the owner is the terminal.
        if self.rename.is_some() {
            let mut editor = self.rename.take().expect("the editor is open");
            let verdict = rename_key(&mut editor, &event.logical_key, self.modifiers);
            self.rename = Some(editor);
            match verdict {
                RenameVerdict::Commit => self.finish_rename(true)?,
                RenameVerdict::Cancel => self.finish_rename(false)?,
                RenameVerdict::Held => {
                    // Typing reveals the caret, exactly as it does in the
                    // terminal — a caret that blinks out from under the letter
                    // you just typed reads as a dropped keystroke.
                    self.rename_blink.reset(now);
                    self.refresh_chrome();
                    self.present_chrome_change()?;
                }
            }
            return Ok(());
        }
        // A popup is not a modal, so it owns exactly one key: the one that puts
        // it away. Everything else is still the terminal's.
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && self.close_profile_menu()?
        {
            return Ok(());
        }

        // A non-empty winit Preedit is the composition authority. Editing/navigation keys are
        // intentionally left to the IME here even if it also exposes a physical named key; no PTY
        // byte may escape this branch and regress M0-beta's composition isolation.
        if self.preedit.is_some() && input::is_ime_owned_key(&event.logical_key, self.modifiers) {
            return Ok(());
        }
        if input::should_copy_selection(
            &event.logical_key,
            self.modifiers,
            self.session.view_selection().is_some(),
        ) {
            if !event.repeat {
                self.copy_selection()?;
            }
            return Ok(());
        }
        if input::is_paste_shortcut(&event.logical_key, self.modifiers) {
            if !event.repeat {
                self.paste_from_clipboard()?;
            }
            return Ok(());
        }
        // Dev-only: open or close the preview seat at its ruled fixed-right
        // address, so the layout can be felt before the verbs that will really
        // open it exist. Ctrl+Alt+Shift+P is a placeholder binding and is
        // documented as such — it wears Alt to leave Ctrl+Shift+P to the command
        // palette the mock-up promises. It is checked here, above the PTY
        // encoder, so the chord never reaches the child.
        if is_preview_toggle_shortcut(&event.logical_key, self.modifiers) {
            if !event.repeat {
                self.toggle_preview_seat()?;
            }
            return Ok(());
        }
        // Dev-only, and above the PTY encoder for the same reason: the chord
        // must not reach the child. Splits the focused terminal pane and spawns
        // the shell that lives in the new one.
        if let Some(dir) = split_shortcut_direction(&event.logical_key, self.modifiers) {
            if !event.repeat {
                self.split_focused_terminal(dir)?;
            }
            return Ok(());
        }
        // The prompt answers Enter with the button it opened focused, and Esc
        // with nothing at all: an unanswered question folds back into
        // `lastSession` (§7.1.4), so Esc must dismiss the *prompt* without
        // deciding for the user. It sits above the PTY encoder so neither key
        // reaches the child while the question is up.
        if self.restore_prompt.is_open() && !self.pending_restore.is_empty() {
            match &event.logical_key {
                Key::Named(NamedKey::Enter) => {
                    if !event.repeat {
                        self.answer_restore_prompt(restore::FOCUSED_ANSWER)?;
                    }
                    return Ok(());
                }
                Key::Named(NamedKey::Escape) if self.restore_prompt.consumes_escape() => {
                    if !event.repeat {
                        self.restore_prompt.close();
                        if self.refresh_chrome() {
                            self.present_chrome_change()?;
                        }
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        // Undo close. It sits above the PTY encoder for the same reason the line
        // above does — the chord is ours, so the child never sees it — and it is
        // below the settings guard, which swallows every key while the dialog is
        // up (mock-up 7374-7376 tests the same two conditions).
        if is_reopen_closed_tab_shortcut(&event.logical_key, self.modifiers) {
            if !event.repeat {
                self.reopen_recent(0)?;
            }
            return Ok(());
        }

        if !self.session.terminal_modes().alternate_screen {
            let page = self.grid.rows.get() as i32;
            match &event.logical_key {
                Key::Named(NamedKey::PageUp) if self.modifiers == ModifiersState::SHIFT => {
                    return self.scroll_view(page);
                }
                Key::Named(NamedKey::PageDown) if self.modifiers == ModifiersState::SHIFT => {
                    return self.scroll_view(-page);
                }
                Key::Named(NamedKey::Home) if self.modifiers == ModifiersState::CONTROL => {
                    self.projection.scroll_to_top();
                    return self.publish_interaction_frame();
                }
                Key::Named(NamedKey::End) if self.modifiers == ModifiersState::CONTROL => {
                    self.projection.scroll_to_bottom();
                    return self.publish_interaction_frame();
                }
                _ => {}
            }
        } else if matches!(&event.logical_key, Key::Named(NamedKey::End))
            && self.modifiers == ModifiersState::CONTROL
            && self.projection.is_scrolled()
        {
            // The application's own jump-to-bottom binding: also return the projection-local
            // overflow review to rest so both scroll layers land at the bottom together. The
            // bytes still reach the application below.
            self.projection.scroll_to_bottom();
            self.publish_interaction_frame()?;
        }

        let application_cursor_mode = self.session.application_cursor_mode();
        let Some(bytes) =
            input::keyboard_bytes(&event.logical_key, self.modifiers, application_cursor_mode)
        else {
            return Ok(());
        };
        self.send_user_input(
            &bytes,
            "write keyboard input to PTY",
            UserInputKind::Keyboard,
        )
    }

    fn paste_from_clipboard(&mut self) -> Result<()> {
        let window = Arc::clone(&self.window);
        let active = self.active_tab;
        // Destructured rather than reached through three derefs: the paste needs
        // the shell's screen, its projection and its pipe held at once, and they
        // are three fields of one leaf.
        let LeafSession {
            pty,
            session,
            projection,
            ..
        } = self.tabs[active].focused_mut();
        if !paste_from_clipboard(
            session,
            projection,
            || {
                window_hwnd(&window).and_then(|hwnd| {
                    bt_platform::clipboard_text(hwnd)
                        .map_err(|error| anyhow!(error))
                        .context("read clipboard text")
                })
            },
            |chunk| {
                if let Some(pty) = pty.as_mut() {
                    pty.write(chunk).context("write clipboard paste to PTY")?;
                }
                Ok(())
            },
        )? {
            return Ok(());
        }
        self.pending_keyboard_at = Some(Instant::now());
        self.publish_frame(FrameTrigger {
            occurred_at: self.pending_keyboard_at.unwrap_or_else(Instant::now),
            source: FrameSource::Keyboard,
        })
    }

    fn ime_input(&mut self, event: Ime) -> Result<()> {
        // The same rule the keyboard follows: with a modal up the terminal is
        // not who is being typed at, and a commit is a keystroke that took a
        // longer road. Enable/disable still pass, so the IME's own bookkeeping
        // stays consistent for when the dialog closes.
        if self.settings_layout().is_some() && matches!(event, Ime::Preedit(..) | Ime::Commit(_)) {
            return Ok(());
        }
        // The name editor takes composed text through the same door typed
        // characters use, so "typing over the opening selection replaces it" is
        // one rule rather than one rule per input method.
        //
        // The pre-edit itself is deliberately *not* drawn in the tab: the
        // ticket scopes the editor to the composition's committed text ("含 IME
        // 组合的落字"), and the candidate window is placed from the terminal's
        // caret, which is not where these letters are going. What must not
        // happen is the pre-edit reaching the terminal underneath, and it does
        // not — the branch returns before `self.preedit` is touched.
        if self.rename.is_some() {
            if let Ime::Commit(text) = &event {
                let mut editor = self.rename.take().expect("the editor is open");
                editor.insert(text);
                self.rename = Some(editor);
                self.rename_blink.reset(Instant::now());
                self.refresh_chrome();
                self.present_chrome_change()?;
            }
            if matches!(event, Ime::Preedit(..) | Ime::Commit(_)) {
                return Ok(());
            }
        }
        if matches!(&event, Ime::Preedit(..) | Ime::Commit(_)) {
            self.reset_cursor_blink(Instant::now());
        }
        match event {
            Ime::Enabled => {
                self.ime_active = true;
                self.ime_cursor_throttle.reset();
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
            Ime::Preedit(text, cursor_range) => {
                // Spike 04 found all three Chinese IMEs report a collapsed caret. Target-clause
                // styling is intentionally outside M0-beta; keep only the caret's start offset.
                self.preedit = (!text.is_empty()).then_some(Preedit {
                    text,
                    cursor_byte: cursor_range.map(|(start, _)| start),
                });
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Keyboard,
                })
            }
            Ime::Commit(text) => {
                self.preedit = None;
                self.return_to_live_for_input();
                self.pending_keyboard_at = Some(Instant::now());
                // IMM32 also emits this commit when focus/layout changes mid-composition. M0-beta
                // deliberately accepts it exactly like Windows Terminal: every commit reaches PTY.
                if let Some(pty) = self.pty.as_mut() {
                    pty.write(&ime_commit_bytes(&text))
                        .context("write IME UTF-8 commit to PTY")?;
                }
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Keyboard,
                })
            }
            Ime::Disabled => {
                self.preedit = None;
                self.ime_active = false;
                self.ime_cursor_throttle.reset();
                self.ime_system_caret.destroy();
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
        }
    }

    fn resize(&mut self, physical: PhysicalSize<u32>) -> Result<()> {
        if physical.width == 0 || physical.height == 0 {
            return Ok(());
        }
        self.defer_preview_resample(Instant::now());
        // Both halves of a visible flyout belong to the viewport that produced them: the anchor is
        // a physical point on the old surface, and the raster was sized to the old pane. A resize
        // dissolves it exactly as a wheel notch does; the retained thumbnail is re-derived, and
        // resampled if the new pane asks for another box, on the next settled hover. The frame
        // this handler publishes below is the repaint that drops it, so nothing is queued here:
        // the frame on screen belongs to the old grid and the resize gate would refuse it.
        self.peek_hover.clear();
        self.renderer.set_peek_overlay(None);
        // The Resized payload is already in physical pixels. Synchronize presentation before any
        // DPI reconciliation can publish a frame, then reconcile once more against inner_size().
        self.renderer
            .resize(physical.width, physical.height)
            .context("synchronize renderer swapchain with resized physical client")?;
        self.reconcile_authoritative_dpi("resized")?;
        let requested_physical = self.window.inner_size();
        if requested_physical.width == 0 || requested_physical.height == 0 {
            return Ok(());
        }
        let presentation = self.renderer.presentation_geometry();
        trace_surface_size_clamp(
            self.trace_resize,
            "BT_RESIZE_TRACE",
            requested_physical,
            presentation,
        );
        let render_physical = presentation_physical_size(presentation);
        let resize_trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Resize,
        };
        // solve -> seat rects -> cols/rows -> the existing 200ms ConPTY quiet
        // coalescing -> LayoutKey. §4.2 fixes this order and forbids the reverse
        // (red line L10); the solver itself takes no part in the debounce.
        let next_grid = self.resolve_seat_layout(render_physical);
        let observed_at = Instant::now();
        // A `Resized` that settles back onto the grid ConPTY already has — the common shape of
        // the very first delivery after a clean, same-DPI session restore — must not schedule a
        // real ConPTY resize at all; see `coalesce_pty_resize_on_grid_change`.
        self.schedule_grid_change(
            next_grid,
            terminal_pty_physical(&self.renderer, render_physical),
            observed_at,
            "resize terminal actor",
        )?;
        self.sync_math_layout_key();
        // The grid actually in force, which under the typed-input gate is still the old one.
        self.pending_resize_present = Some(self.grid);
        self.publish_frame(resize_trigger)?;
        // Windows dispatches Resized from its modal move/size loop. `Renderer::resize` only records
        // the requested swapchain geometry; `present` prepares this newly projected frame first,
        // then performs ResizeBuffers immediately before acquire/submit. Thus the handler exposes
        // no intermediate "new surface + old grid" frame. Until this callback completes, DWM may
        // scale the previous complete back buffer as one image, which is the all-frame fallback.
        self.redraw()
    }

    fn scale_factor_changed(&mut self) -> Result<()> {
        self.defer_preview_resample(Instant::now());
        self.reconcile_authoritative_dpi("scale-factor-changed")?;
        self.resize(self.window.inner_size())
    }

    fn reconcile_authoritative_dpi(&mut self, stage: &'static str) -> Result<bool> {
        let physical = self.window.inner_size();
        if physical.width > 0 && physical.height > 0 {
            self.renderer
                .resize(physical.width, physical.height)
                .context("reconcile swapchain with physical client size")?;
            ensure_swapchain_matches_inner(&self.renderer, physical)?;
        }
        let render_physical = presentation_physical_size(self.renderer.presentation_geometry());
        // Touching the surface is what obliges a solve, not changing the DPI:
        // every seat rectangle is a function of the surface, so the answer that
        // was true of the old one is not yet true of this one. The equal-scale
        // path below returns early and this method publishes a frame, so the
        // solve has to happen here rather than after that branch — otherwise the
        // frame is drawn against a rectangle nobody re-derived, which is how the
        // terminal came to be drawn over its neighbour's seat. `solve` is pure
        // and the tree has not changed, so on the common no-op path this is the
        // same answer arrived at again.
        if physical.width > 0 && physical.height > 0 {
            self.resolve_seat_layout(render_physical);
        }
        let snapshot = dpi_snapshot(&self.window)?;
        log_dpi_snapshot(
            stage,
            snapshot,
            Some(self.renderer.metrics().scale_factor),
            self.renderer.presentation_geometry(),
            physical,
        );
        if scale_factors_match(
            self.renderer.metrics().scale_factor,
            snapshot.authoritative_scale,
        ) {
            return Ok(false);
        }

        self.apply_scale_factor(snapshot.authoritative_scale)?;
        if physical.width > 0 && physical.height > 0 {
            // A DPI change is a similarity transform: the same tree solved again
            // on a new rectangle. Red line L5 — not one ratio, not one fixed
            // extent is rewritten on this path, and `resolve_seat_layout` writes
            // none because `solve` is pure.
            self.refresh_work_area();
            self.apply_window_min_inner_size()?;
            let next_grid = self.resolve_seat_layout(render_physical);
            self.schedule_grid_change(
                next_grid,
                terminal_pty_physical(&self.renderer, render_physical),
                Instant::now(),
                "rebuild terminal grid after authoritative DPI correction",
            )?;
        }
        self.sync_math_layout_key();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    fn sync_math_layout_key(&mut self) {
        // Future runtime theme switching has one required hook: update renderer theme colors, then
        // call this method. `LayoutKey` contains `theme_rev`, so a theme change must invalidate old
        // textures; the revision enters both the worker gate and GPU texture identity. The session
        // keeps same-source old pixels only while the replacement is pending.
        let width_cells = nonzero_u32(self.grid.columns.get());
        let dpi_milli = self.renderer.metrics().dpi_milli();
        self.session.set_layout_key(LayoutKey {
            width_cells,
            dpi_milli,
            font_rev: 1,
            theme_rev: theme_revision(),
        });
    }

    fn apply_scale_factor(&mut self, scale_factor: f64) -> Result<()> {
        let metrics = self
            .renderer
            .update_scale_factor(scale_factor)
            .context("remeasure terminal font at new DPI")?;
        ensure_metrics_match_authoritative_scale(metrics.scale_factor, scale_factor)?;
        // Every shell in every tab: a DPI change is a fact about the display, so
        // no screen anywhere in the window is exempt from it.
        for tab in &mut self.tabs {
            for (_, leaf) in tab.leaves_mut() {
                leaf.session
                    .set_cell_height_subpixels(metrics.cell_height_subpixels());
                leaf.session
                    .set_cell_width_subpixels(cell_width_subpixels(metrics));
                leaf.session
                    .set_ascii_baseline_subpixels(metrics.ascii_baseline_subpixels());
            }
        }
        Ok(())
    }

    /// Retire shells that have exited, and the panes and tabs they emptied.
    ///
    /// Two levels now, because a shell exiting is a fact about a *pane*. A tab
    /// whose right-hand pane's shell exits loses that pane and keeps running;
    /// only when a tab has no live shell left has the tab itself ended. That is
    /// the same rule §7.1.4 already gives closing — the last pane closing is the
    /// tab closing — read from the other direction.
    fn reap_exited_tabs(&mut self) -> Result<()> {
        // Which panes of the *active* tab died: those are the ones that can be
        // closed as panes, because `close_pane` re-solves the tab the user is
        // looking at.
        let active = self.active_tab;
        let mut exited_panes = Vec::new();
        for (seat, leaf) in self.tabs[active].leaves_mut() {
            let Some(pty) = leaf.pty.as_mut() else {
                continue;
            };
            if pty.try_wait()?.is_some() {
                exited_panes.push(*seat);
            }
        }
        // Never close the last one here: an empty tab is not a state, and
        // `close_pane` routes that case to `close_tab` on its own.
        if self.tabs[active].sessions.len() > exited_panes.len() {
            for seat in exited_panes {
                self.close_pane(seat)?;
            }
        }

        let mut exited = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            // A tab has ended when every shell it holds has ended. A tab in
            // probe mode holds no PTY at all and never ends this way.
            let mut any_live = false;
            let mut any_pty = false;
            for (_, leaf) in tab.leaves_mut() {
                let Some(pty) = leaf.pty.as_mut() else {
                    continue;
                };
                any_pty = true;
                if pty.try_wait()?.is_none() {
                    any_live = true;
                }
            }
            if any_pty && !any_live {
                exited.push(index);
            }
        }
        for index in exited.into_iter().rev() {
            self.close_tab(index)?;
            if self.tabs.len() == 1 && index == 0 {
                break;
            }
        }
        Ok(())
    }

    fn redraw(&mut self) -> Result<()> {
        let Some((frame, trigger)) = self.pending_frames.take() else {
            return Ok(());
        };
        if let Some(expected) = self.pending_resize_present {
            ensure!(
                frame_matches_grid(&frame, expected),
                "resize presentation requires the newly projected grid: expected {}x{}, got {}x{}",
                expected.columns,
                expected.rows,
                frame.columns,
                frame.grid_rows
            );
        }
        let has_text = frame
            .cells
            .iter()
            .take(
                frame
                    .drawable_rows()
                    .saturating_mul(frame.columns.get() as usize),
            )
            .any(|cell| !cell.text.trim().is_empty());
        // The other panes of this tab. The focused leaf's frame came out of the
        // slot above, with every presentation-hold and resize contract the slot
        // exists to enforce still attached to it; the panes the user is not
        // typing in hold nothing, so they are projected here, at the moment they
        // are drawn.
        //
        // A lone terminal leaf produces exactly one entry, whose rectangle is
        // the same `pane_body_viewport` answer `resolve_seat_layout` already
        // handed the renderer — so the slice is N = 1 and the command stream is
        // the one that was always issued. The loop is not a second path; it is
        // the same path counted.
        let focused_leaf = self.focused_leaf;
        let scale = self.renderer.metrics().scale_factor as f32;
        let bodies: Vec<(bt_layout::SeatId, bt_render::SeatViewport)> = self
            .seats
            .terminals()
            .into_iter()
            .filter_map(|seat| {
                seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)
                    .map(|body| (seat, body))
            })
            .collect();
        let active = self.active_tab;
        let mut unfocused_frames: Vec<(bt_layout::SeatId, bt_render::SeatViewport, ViewportFrame)> =
            Vec::new();
        for (seat, body) in &bodies {
            if *seat == focused_leaf {
                continue;
            }
            let Some(leaf) = self.tabs[active].sessions.get_mut(seat) else {
                continue;
            };
            leaf.session.refresh_projection(&mut leaf.projection);
            let projected = leaf
                .session
                .viewport_frame(&mut leaf.projection)
                .context("project an unfocused pane's grid into a viewport frame")?;
            unfocused_frames.push((*seat, *body, projected));
        }
        let focused_body = bodies
            .iter()
            .find(|(seat, _)| *seat == focused_leaf)
            .map(|(_, body)| *body)
            .unwrap_or_else(|| self.renderer.seat_viewport());
        let mut seat_frames = Vec::with_capacity(unfocused_frames.len() + 1);
        seat_frames.push(bt_render::SeatFrame {
            seat: focused_body,
            frame: &frame,
            focused: true,
        });
        for (_, body, projected) in &unfocused_frames {
            seat_frames.push(bt_render::SeatFrame {
                seat: *body,
                frame: projected,
                focused: false,
            });
        }
        match self
            .renderer
            .present_seats(&seat_frames, trigger)
            .context("render terminal frame")?
        {
            PresentOutcome::Presented(receipt) => {
                if self.window_shown && !self.first_visible_present_dpi_checked {
                    self.first_visible_present_dpi_checked = true;
                    self.reconcile_authoritative_dpi("first-present")?;
                }
                let latency = receipt.latency();
                if self.trace_startup
                    && matches!(trigger.source, FrameSource::Resize)
                    && let Ok(latency) = latency
                {
                    eprintln!(
                        "BT_RESIZE present={}us columns={} rows={}",
                        latency.event_to_present_call.as_micros(),
                        frame.columns,
                        frame.grid_rows
                    );
                }
                if has_text && !self.first_text_presented {
                    self.first_text_presented = true;
                    if self.trace_startup {
                        let text_visible = self.startup_started.elapsed();
                        self.first_text_visible = Some(text_visible);
                        eprintln!(
                            "BT_STARTUP first_text_present={}ms",
                            text_visible.as_millis()
                        );
                    }
                }
                // Each pane keeps the frame it just drew, so a pointer question
                // asked over it can be answered by its own cells. The focused
                // leaf's copy is `Runtime::last_presented_frame` as well, which
                // is what the presentation-hold and scroll contracts read.
                let active = self.active_tab;
                for (seat, _, projected) in unfocused_frames {
                    if let Some(leaf) = self.tabs[active].sessions.get_mut(&seat) {
                        leaf.last_presented_frame = Some(projected);
                    }
                }
                if let Some(leaf) = self.tabs[active].sessions.get_mut(&focused_leaf) {
                    leaf.last_presented_frame = Some(frame.clone());
                }
                self.last_presented_frame = Some(frame);
                self.pending_resize_present = None;
            }
            PresentOutcome::Skipped | PresentOutcome::Reconfigure => {
                self.pending_frames
                    .publish(frame, trigger)
                    .context("reject non-rectangular frame during redraw retry")?;
                self.window.request_redraw();
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        // §7.1.4: "未提交的重命名在序列化前提交（blur 语义,输入到一半关窗不丢
        // 新名字）". Before the snapshot below, not after — the name has to be on
        // the tab by the time the tab is written down.
        self.finish_rename(true)?;
        // The clean-exit path (§5.5): flush whatever the debounce still owes,
        // then drop this run's sentinel. Its absence next time is the whole
        // signal that this run reached here at all, so it must be removed
        // before anything below is allowed to fail.
        self.mark_session_dirty(Instant::now());
        self.session_store.close();
        self.ime_system_caret.destroy();
        for tab in &mut self.tabs {
            for (_, leaf) in tab.leaves_mut() {
                if let Some(pty) = leaf.pty.as_mut() {
                    pty.shutdown().context("shut down child process")?;
                }
            }
        }
        Ok(())
    }
}

struct BetterTerminalApp {
    runtime: Option<Runtime>,
    proxy: EventLoopProxy<AppEvent>,
    startup_started: Instant,
}

impl BetterTerminalApp {
    fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            runtime: None,
            proxy,
            startup_started: Instant::now(),
        }
    }
}

impl BetterTerminalApp {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        eprintln!("BetterTerminal stopped: {error:#}");
        if let Some(runtime) = self.runtime.as_mut()
            && let Err(shutdown_error) = runtime.shutdown()
        {
            eprintln!("child shutdown also failed: {shutdown_error:#}");
        }
        self.runtime = None;
        event_loop.exit();
    }
}

impl ApplicationHandler<AppEvent> for BetterTerminalApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.runtime.is_some() {
            return;
        }
        match Runtime::create(event_loop, self.proxy.clone(), self.startup_started) {
            Ok(runtime) => {
                self.runtime = Some(runtime);
                // Output can already be buffered after the long GPU initialization path. Drain
                // once after installing Runtime instead of waiting for another event-loop turn.
                if let Some(runtime) = self.runtime.as_mut()
                    && let Err(error) = runtime.drain_pty()
                {
                    self.fail(event_loop, error);
                    return;
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + STARTUP_PTY_POLL_INTERVAL,
                ));
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::PtyOutput => {
                if let Some(runtime) = self.runtime.as_mut()
                    && let Err(error) = runtime.drain_pty()
                {
                    self.fail(event_loop, error);
                }
            }
            AppEvent::MathReady => {
                if let Some(runtime) = self.runtime.as_mut()
                    && let Err(error) = runtime.apply_math_results()
                {
                    self.fail(event_loop, error);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        if runtime.window.id() != window_id {
            return;
        }
        let result = match event {
            WindowEvent::CloseRequested => {
                let result = runtime.shutdown();
                self.runtime = None;
                event_loop.exit();
                result
            }
            WindowEvent::KeyboardInput { event, .. } => runtime.keyboard_input(&event),
            WindowEvent::Ime(event) => runtime.ime_input(event),
            WindowEvent::ModifiersChanged(modifiers) => {
                runtime.modifiers = modifiers.state();
                Ok(())
            }
            WindowEvent::CursorMoved { position, .. } => runtime.pointer_moved(position),
            WindowEvent::CursorLeft { .. } => runtime.pointer_left(),
            WindowEvent::MouseInput { state, button, .. } => runtime.mouse_input(state, button),
            WindowEvent::MouseWheel { delta, .. } => runtime.mouse_wheel(delta),
            WindowEvent::Resized(size) => runtime.resize(size),
            WindowEvent::ScaleFactorChanged { .. } => runtime.scale_factor_changed(),
            WindowEvent::ThemeChanged(theme) => runtime.os_theme_changed(theme).map(|_| ()),
            WindowEvent::RedrawRequested => runtime.redraw(),
            WindowEvent::Focused(false) => {
                // Losing the window is a blur, and blur commits (J102). The
                // mock-up's editor is a real focusable element and gets this
                // from the DOM; here it has to be said. A press that was still
                // being held goes with it — the button-up will arrive to a
                // window that is no longer listening.
                let committed = runtime.finish_rename(true);
                // K129. Losing the window is losing the Win32 mouse capture, and
                // capture loss is this platform's `pointercancel`: the pointer
                // stream ends with no button-up to end it. It is the only such
                // signal winit surfaces — there is no `WM_CAPTURECHANGED` event —
                // and an un-torn-down drag would leave a tab floating over a strip
                // nobody is holding any more. Same path as Esc: never mind.
                // F72: the same capture loss ends a divider drag, and ends it
                // the same way — restoring the one ratio it had been moving.
                // Left holding the button over a window that is no longer
                // listening, the gesture was never finished, and a drag nobody
                // finished did not choose a ratio.
                let cancelled = runtime
                    .cancel_drag()
                    .and_then(|tab| runtime.cancel_divider_drag().map(|split| tab || split));
                runtime.tab_press = None;
                runtime.pane_press = None;
                runtime.tab_clicks.interrupt();
                // Do not cancel or synthesize anything: IMM32 may synchronously deliver a partial
                // Commit during this transition, and the product decision is to accept it.
                runtime.ime_active = false;
                runtime.ime_cursor_throttle.reset();
                runtime.ime_system_caret.destroy();
                runtime.set_cursor_focus(false, Instant::now());
                committed.and(cancelled.map(|_| ())).and_then(|()| {
                    runtime.publish_frame(FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::Expose,
                    })
                })
            }
            WindowEvent::Focused(true) => {
                runtime.set_cursor_focus(true, Instant::now());
                runtime.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
            WindowEvent::Occluded(false) => runtime.publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            }),
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        runtime.apply_math_context_menu_result();
        if let Err(error) = runtime.drain_pty() {
            self.fail(event_loop, error);
            return;
        }
        let now = Instant::now();
        if let Err(error) = runtime.advance_cursor_blink_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.advance_rename_blink_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        // Ahead of the strip's own animation tick: paying the press's promise
        // activates a tab, and the strip that is redrawn afterwards should be
        // the one the switch produced.
        if let Err(error) = runtime.advance_tab_press_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.advance_strip_animation(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.finish_synchronized_update_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        runtime.session_store.flush_if_due(now);
        runtime.flush_ime_cursor_area(now);
        if let Err(error) = runtime.finish_resize_if_quiescent(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.finish_preview_resize_if_quiet(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.advance_live_math_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.activate_hyperlink_hover_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.activate_peek_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.clear_math_hover_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        // Ahead of the tip's own promotion, so §6's ordering is structural and
        // not merely a consequence of 350 being less than 380: on the frame both
        // came due, the peek is already showing when the tip asks.
        if let Err(error) = runtime.advance_layout_peek_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.advance_tooltip_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        // Service the PTY gate after every other due task that can mutate session state, then carry
        // the deadline derived from that exact sample into the control-flow decision below.
        let pty_resize_deadline = match runtime.flush_pending_pty_resize(now) {
            Ok(deadline) => deadline,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        let startup_deadline =
            startup_poll_delay(runtime.first_text_presented).map(|delay| now + delay);
        let resize_finish_deadline = runtime
            .tabs
            .iter()
            .filter_map(|tab| tab.session.resize_finish_deadline())
            .min();
        let synchronized_update_deadline = runtime
            .tabs
            .iter()
            .filter_map(|tab| tab.session.synchronized_update_deadline())
            .min();
        let live_stability_deadline = runtime.session.live_stability_deadline();
        let wake_deadline = earliest_deadline([
            startup_deadline,
            runtime.ime_cursor_throttle.deadline(),
            runtime.cursor_blink.deadline(),
            // The press's own 180ms, and only while it still owes one — a press
            // that has been paid or has slipped reports nothing, so a held
            // button costs no wake-ups at all.
            runtime.tab_press.as_ref().and_then(TabPress::wake_deadline),
            // The rename caret blinks only while there is a rename.
            runtime
                .rename
                .is_some()
                .then(|| runtime.rename_blink.deadline())
                .flatten(),
            runtime.strip_animation_deadline(now),
            pty_resize_deadline,
            resize_finish_deadline,
            synchronized_update_deadline,
            live_stability_deadline,
            // The tip's 380ms while one is settling, and the fade's own frames
            // until it lands. A window with no tip under the pointer reports
            // nothing and costs no wake-ups at all.
            runtime.tooltip_deadline(now),
            // The peek's 350ms while one is settling, and nothing afterwards:
            // it has no fade, so a schematic on screen is finished and asks for
            // no frames at all.
            runtime.layout_peek.deadline(),
            runtime.hyperlink_hover.show_at,
            runtime.peek_hover.show_at,
            runtime.math_hover_clear_at,
            runtime
                .preview_image
                .as_ref()
                .and_then(|preview| preview.resize_scale_deadline),
            runtime.session_store.deadline(),
        ]);
        event_loop
            .set_control_flow(wake_deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
        if let Err(error) = runtime.reap_exited_tabs() {
            self.fail(event_loop, error);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(runtime) = self.runtime.as_mut()
            && let Err(error) = runtime.shutdown()
        {
            eprintln!("child shutdown failed: {error:#}");
        }
        self.runtime = None;
    }
}

fn ime_commit_bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn copy_selection(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    write: impl FnOnce(&str) -> Result<()>,
) -> bool {
    if !write_selection_text(session, false, write) {
        return false;
    }
    session.set_view_selection(None);
    projection.set_selection(None);
    true
}

fn write_selection_text(
    session: &DualPlaneSession,
    ignore_empty: bool,
    write: impl FnOnce(&str) -> Result<()>,
) -> bool {
    let Some(text) = session.selection_text() else {
        return false;
    };
    if ignore_empty && text.is_empty() {
        return false;
    }
    if !recoverable_clipboard_write(write(&text), "copy") {
        return false;
    }
    true
}

fn write_terminal_clipboard_text(window: &Window, text: &str) -> Result<()> {
    window_hwnd(window).and_then(|hwnd| {
        bt_platform::set_clipboard_text(hwnd, text)
            .map_err(|error| anyhow!(error))
            .context("write terminal selection to clipboard")
    })
}

fn recoverable_clipboard_write(result: Result<()>, action: &str) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            eprintln!("clipboard is temporarily unavailable; {action} ignored: {error:#}");
            false
        }
    }
}

fn paste_from_clipboard(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    read: impl FnOnce() -> Result<String>,
    mut write: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    let Some(text) = recoverable_clipboard_read(read()) else {
        return Ok(false);
    };
    let bytes = input::paste_bytes(&text, session.bracketed_paste_mode());
    session.set_view_selection(None);
    projection.set_selection(None);
    projection.scroll_to_bottom();
    // Keep paste on the sole synchronous PTY writer. Fixed-size writes let ConPTY's existing
    // write_all/flush path apply backpressure instead of one unbounded call.
    for chunk in bytes.chunks(input::PASTE_WRITE_CHUNK_BYTES) {
        write(chunk)?;
    }
    Ok(true)
}

fn recoverable_clipboard_read(result: Result<String>) -> Option<String> {
    match result {
        Ok(text) => Some(text),
        Err(error) => {
            eprintln!("clipboard does not contain readable text; paste ignored: {error:#}");
            None
        }
    }
}

fn recoverable_wheel_scroll_amount(
    result: std::result::Result<bt_platform::WheelScrollAmount, String>,
) -> bt_platform::WheelScrollAmount {
    match result {
        Ok(amount) => amount,
        Err(error) => {
            const FALLBACK_WHEEL_LINES: u32 = 3;
            eprintln!(
                "system wheel scroll setting unavailable; using {FALLBACK_WHEEL_LINES} lines: {error}"
            );
            bt_platform::WheelScrollAmount::Lines(FALLBACK_WHEEL_LINES)
        }
    }
}

/// Takes as many whole `unit`s as the accumulated remainder holds and returns the signed count,
/// leaving the sub-unit residue in place. Truncation is symmetric around zero, so reversing the
/// wheel never manufactures motion from residue of the opposite sign.
fn drain_whole_units(remainder: &mut f64, unit: f64) -> i64 {
    let units = (*remainder / unit).trunc() as i64;
    *remainder -= units as f64 * unit;
    units
}

fn protocol_mouse_button(button: MouseButton) -> Option<input::MouseProtocolButton> {
    match button {
        MouseButton::Left => Some(input::MouseProtocolButton::Left),
        MouseButton::Middle => Some(input::MouseProtocolButton::Middle),
        MouseButton::Right => Some(input::MouseProtocolButton::Right),
        MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => None,
    }
}

fn startup_poll_delay(first_text_presented: bool) -> Option<std::time::Duration> {
    (!first_text_presented).then_some(STARTUP_PTY_POLL_INTERVAL)
}

/// The longest a tab's name may be: `TITLE_MAX` (`design/ui-mockup.html` line
/// 2603).
///
/// Counted in characters rather than the mock-up's UTF-16 code units, which is
/// the same number for everything that is not an astral plane character and the
/// more honest reading of "40 characters" for the rest.
const TITLE_MAX_CHARS: usize = 40;

/// `cleanTitle` — the mock-up's own sanitiser, applied to text this terminal did
/// not write.
///
/// Strip the C0 and C1 control ranges, trim, cap. The reason is stated at the
/// mock-up's own definition: a program-controlled title is untrusted input, and
/// "in the product it must also never be able to impersonate chrome". A title
/// carrying a newline, a backspace or a C1 escape introducer is a title that can
/// redraw the strip around it.
fn clean_title(text: &str) -> String {
    text.chars()
        .filter(|character| !matches!(character, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}'))
        .collect::<String>()
        .trim()
        .chars()
        .take(TITLE_MAX_CHARS)
        .collect()
}

/// `cwdLeaf` — "what to show when there is room for one word: the folder you are
/// in" (mock-up line 2585).
///
/// Ported as the mock-up writes it, over the path's own text rather than through
/// `Path::file_name`, because the two disagree exactly where it matters: the
/// mock-up's `"C:\\".replace(/[\\/]+$/,"").split(...)` yields `C:`, while
/// `file_name` yields nothing at a drive root. A drive root is a place you can
/// stand, so it gets a name.
fn cwd_leaf(directory: &Path) -> Option<String> {
    let text = directory.to_str()?;
    let trimmed = text.trim_end_matches(['\\', '/']);
    let leaf = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    let leaf = if leaf.is_empty() { trimmed } else { leaf };
    (!leaf.is_empty()).then(|| leaf.to_owned())
}

/// The tooltip anchor a window-chrome target answers to, for the four the
/// caption run is made of.
///
/// A function rather than a `From`, because it is deliberately partial: most of
/// [`seats::ChromeTarget`] names something a click does that nothing hovers for
/// (a divider, a pane head), and the two enums are separate precisely so neither
/// has to grow entries for the other's questions.
fn tooltip_anchor_for(target: seats::ChromeTarget) -> Option<tooltip::TooltipAnchorId> {
    match target {
        seats::ChromeTarget::Settings => Some(tooltip::TooltipAnchorId::Settings),
        seats::ChromeTarget::Minimize => Some(tooltip::TooltipAnchorId::Minimize),
        seats::ChromeTarget::Maximize => Some(tooltip::TooltipAnchorId::Maximize),
        seats::ChromeTarget::CloseWindow => Some(tooltip::TooltipAnchorId::CloseWindow),
        _ => None,
    }
}

/// A tab's name: 手动 > 程序标题 (OSC 2) > cwd (OSC 7) > the profile's own name.
///
/// "Each layer is more specific than the one under it, and each is something
/// someone actually said: you typed it, the program announced it, or the shell
/// reported where it stands" (mock-up line 2593). Nothing here is inferred.
///
/// Precedence is decided on the *sanitised* layers, not the raw ones, and that
/// is a **ruling** the mock-up does not have to make because nothing in it is
/// hostile. A program that sets its title to a lone control character has said
/// nothing; if the raw value decided precedence, that program could blank a tab,
/// which is precisely the impersonation the sanitiser exists to refuse. An empty
/// answer therefore falls through to the layer beneath it.
fn display_title(
    manual_name: Option<&str>,
    program_title: Option<&str>,
    working_directory: Option<&Path>,
) -> String {
    resolve_title(manual_name, program_title, working_directory).0
}

/// The same walk, keeping the answer to "which layer won?" that
/// [`display_title`] throws away.
///
/// One function with two readers rather than two functions agreeing: the tab's
/// tooltip states the provenance out loud (M140, mock-up 4197-4199), and a
/// second walk of the same stack would be a second set of precedence rules to
/// keep in step — including the sanitiser's fall-through, which is exactly the
/// subtlety a copy would lose. `None` is the fourth layer, which is nobody's
/// claim: the profile's own name is what a tab is called when no one has said
/// anything about it, and it has no provenance to report.
fn resolve_title(
    manual_name: Option<&str>,
    program_title: Option<&str>,
    working_directory: Option<&Path>,
) -> (String, Option<tooltip::NameSource>) {
    let layer = |text: Option<String>| {
        text.map(|text| clean_title(&text))
            .filter(|text| !text.is_empty())
    };
    let tagged = |text: Option<String>, source| layer(text).map(|text| (text, Some(source)));
    tagged(manual_name.map(str::to_owned), tooltip::NameSource::Manual)
        .or_else(|| {
            tagged(
                program_title.map(str::to_owned),
                tooltip::NameSource::Program,
            )
        })
        .or_else(|| {
            tagged(
                working_directory.and_then(cwd_leaf),
                tooltip::NameSource::Cwd,
            )
        })
        .unwrap_or_else(|| (DEFAULT_PROFILE_TITLE.to_owned(), None))
}

#[cfg(test)]
fn startup_scale_title(scale_factor: f64) -> String {
    format!("{WINDOW_TITLE} · {}x", display_scale_factor(scale_factor))
}

#[cfg(test)]
fn startup_trace_title(
    background_visible: Duration,
    first_text_visible: Duration,
    scale_factor: f64,
) -> String {
    format!(
        "{WINDOW_TITLE} — bg {}ms · text {}ms · {}x",
        background_visible.as_millis(),
        first_text_visible.as_millis(),
        display_scale_factor(scale_factor),
    )
}

fn display_scale_factor(scale_factor: f64) -> String {
    format!("{scale_factor:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn scale_factors_match(left: f64, right: f64) -> bool {
    (left - right).abs() <= f64::EPSILON
}

fn ensure_metrics_match_authoritative_scale(
    metrics_scale: f64,
    authoritative_scale: f64,
) -> Result<()> {
    ensure!(
        scale_factors_match(metrics_scale, authoritative_scale),
        "terminal metrics scale factor {metrics_scale} does not match authoritative Win32 scale factor {authoritative_scale}"
    );
    Ok(())
}

fn dpi_snapshot(window: &Window) -> Result<DpiSnapshot> {
    let hwnd = window_hwnd(window)?;
    let win32_dpi = bt_platform::get_dpi_for_window(hwnd)
        .map_err(|error| anyhow!(error))
        .context("query authoritative window DPI with GetDpiForWindow")?;
    let rect = bt_platform::get_window_rect(hwnd)
        .map_err(|error| anyhow!(error))
        .context("query native window rectangle for DPI diagnostics")?;
    Ok(DpiSnapshot {
        winit_scale: window.scale_factor(),
        win32_dpi,
        authoritative_scale: f64::from(win32_dpi) / WIN32_DEFAULT_DPI,
        rect,
    })
}

fn log_dpi_snapshot(
    stage: &str,
    snapshot: DpiSnapshot,
    metrics_scale: Option<f64>,
    presentation: bt_render::PresentationGeometry,
    inner_size: PhysicalSize<u32>,
) {
    let metrics_scale = metrics_scale.map_or_else(|| "n/a".to_owned(), display_scale_factor);
    eprintln!(
        "BT_DPI stage={stage} winit_scale={} win32_dpi={} authoritative_scale={} metrics_scale={} rect={},{},{},{} swapchain_size={}x{} inner_size={}x{}",
        display_scale_factor(snapshot.winit_scale),
        snapshot.win32_dpi,
        display_scale_factor(snapshot.authoritative_scale),
        metrics_scale,
        snapshot.rect.left,
        snapshot.rect.top,
        snapshot.rect.right,
        snapshot.rect.bottom,
        presentation.swapchain_size.0,
        presentation.swapchain_size.1,
        inner_size.width,
        inner_size.height,
    );
}

fn ensure_swapchain_matches_inner(
    renderer: &Renderer,
    inner_size: PhysicalSize<u32>,
) -> Result<()> {
    let presentation = renderer.presentation_geometry();
    let swapchain_size = presentation.swapchain_size;
    ensure!(
        swapchain_size_matches_inner(
            swapchain_size,
            inner_size,
            presentation.max_texture_dimension_2d,
        ),
        "swapchain size {}x{} does not match clamped winit physical inner size {}x{} (device limit {})",
        swapchain_size.0,
        swapchain_size.1,
        inner_size.width,
        inner_size.height,
        presentation.max_texture_dimension_2d,
    );
    Ok(())
}

fn swapchain_size_matches_inner(
    swapchain_size: (u32, u32),
    inner_size: PhysicalSize<u32>,
    max_texture_dimension_2d: u32,
) -> bool {
    let limit = max_texture_dimension_2d.max(1);
    swapchain_size
        == (
            inner_size.width.max(1).min(limit),
            inner_size.height.max(1).min(limit),
        )
}

fn presentation_physical_size(presentation: bt_render::PresentationGeometry) -> PhysicalSize<u32> {
    PhysicalSize::new(presentation.swapchain_size.0, presentation.swapchain_size.1)
}

fn trace_surface_size_clamp(
    enabled: bool,
    prefix: &str,
    requested: PhysicalSize<u32>,
    presentation: bt_render::PresentationGeometry,
) {
    if !enabled
        || (requested.width <= presentation.swapchain_size.0
            && requested.height <= presentation.swapchain_size.1)
    {
        return;
    }
    eprintln!(
        "{prefix} surface_size_clamped requested={}x{} configured={}x{} max_texture_dimension_2d={}",
        requested.width,
        requested.height,
        presentation.swapchain_size.0,
        presentation.swapchain_size.1,
        presentation.max_texture_dimension_2d,
    );
}

fn install_theme_class_background(window: &Window) -> Result<()> {
    bt_platform::install_window_class_background(window_hwnd(window)?, background_rgb())
        .map_err(|error| anyhow!(error))
        .context("install theme-colored winit class background brush")
}

fn render_theme_mode(theme: SessionThemeV1) -> ThemeModeV1 {
    match theme {
        SessionThemeV1::System => ThemeModeV1::System,
        SessionThemeV1::Light => ThemeModeV1::Light,
        SessionThemeV1::Dark => ThemeModeV1::Dark,
    }
}

fn session_theme_mode(mode: ThemeModeV1) -> SessionThemeV1 {
    match mode {
        ThemeModeV1::System => SessionThemeV1::System,
        ThemeModeV1::Light => SessionThemeV1::Light,
        ThemeModeV1::Dark => SessionThemeV1::Dark,
    }
}

fn resolve_theme_mode(mode: ThemeModeV1, os_theme: Option<OsTheme>) -> Theme {
    match mode {
        ThemeModeV1::System => match os_theme {
            Some(OsTheme::Light) => Theme::Light,
            Some(OsTheme::Dark) | None => Theme::Dark,
        },
        ThemeModeV1::Light => Theme::Light,
        ThemeModeV1::Dark => Theme::Dark,
    }
}

fn resolved_theme_change(mode: ThemeModeV1, os_theme: OsTheme) -> Option<Theme> {
    match mode {
        ThemeModeV1::System => Some(resolve_theme_mode(mode, Some(os_theme))),
        ThemeModeV1::Light | ThemeModeV1::Dark => None,
    }
}

fn render_cursor_style(style: SessionCursorStyleV1) -> CursorStyle {
    match style {
        SessionCursorStyleV1::Bar => CursorStyle::Bar,
        SessionCursorStyleV1::Block => CursorStyle::Block,
        SessionCursorStyleV1::Underline => CursorStyle::Underline,
    }
}

fn session_cursor_style(style: CursorStyle) -> SessionCursorStyleV1 {
    match style {
        CursorStyle::Bar => SessionCursorStyleV1::Bar,
        CursorStyle::Block => SessionCursorStyleV1::Block,
        CursorStyle::Underline => SessionCursorStyleV1::Underline,
    }
}

fn window_hwnd(window: &Window) -> Result<std::num::NonZeroIsize> {
    let handle = window.window_handle().context("get Win32 window handle")?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(anyhow!("bt-app requires a Win32 window handle"));
    };
    Ok(handle.hwnd)
}

fn cell_width_subpixels(metrics: bt_render::CellMetrics) -> NonZeroI64 {
    let value = (metrics.cell_width_px * bt_viewport::SUBPIXELS_PER_PX as f32).round() as i64;
    NonZeroI64::new(value.max(1)).expect("cell width is clamped above zero")
}

fn nonzero_u32(value: u16) -> NonZeroU32 {
    NonZeroU32::new(u32::from(value)).expect("grid dimensions originate from NonZeroU16")
}

fn frame_matches_grid(frame: &ViewportFrame, grid: GridSize) -> bool {
    frame.columns.get() == u32::from(grid.columns.get())
        && frame.grid_rows.get() == u32::from(grid.rows.get())
}

fn presentation_equivalent(previous: &ViewportFrame, next: &ViewportFrame) -> bool {
    previous.columns == next.columns
        && previous.grid_rows == next.grid_rows
        && previous.rows == next.rows
        && previous.presentation_offset_subpixels == next.presentation_offset_subpixels
        && previous.cells == next.cells
        && previous.cursor == next.cursor
        && previous.cell_anchors == next.cell_anchors
        && previous.row_map == next.row_map
        && previous.selection_spans == next.selection_spans
        && previous.math_blocks == next.math_blocks
        && previous.status_text == next.status_text
        && previous.viewport_origin == next.viewport_origin
        && previous.scroll_offset_rows == next.scroll_offset_rows
        && previous.layout_key == next.layout_key
}

fn pty_frame_is_unchanged(
    pending: Option<&ViewportFrame>,
    last_presented: Option<&ViewportFrame>,
    next: &ViewportFrame,
) -> bool {
    pending
        .or(last_presented)
        .is_some_and(|previous| presentation_equivalent(previous, next))
}

/// The dev-only preview toggle: `Ctrl+Alt+Shift+P`.
///
/// Matched on the *character* the layout produced rather than on a physical key
/// so it behaves the same on every keyboard layout, and required to carry the
/// whole chord and nothing else — a bare `Ctrl+P` is a real terminal control
/// byte (DLE) and must keep reaching the child.
///
/// Alt is in the chord because `Ctrl+Shift+P` is spoken for: the mock-up gives
/// it to the command palette (`design/ui-mockup.html` line 5988), and that
/// binding tests `!e.altKey`, so adding Alt is precisely how a placeholder gets
/// out of the way of the verb that is coming. This toggle is scaffolding for
/// feeling the preview seat's ruled address before the real verbs exist; the
/// palette is product, and product wins the shorter chord.
fn is_preview_toggle_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers == ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SHIFT
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("p"))
}

/// The pane splits: `Ctrl+Alt+Shift+D` beside the focused pane,
/// `Ctrl+Alt+Shift+E` under it.
///
/// Scaffolding of exactly the kind [`is_preview_toggle_shortcut`] is, and
/// documented as such: U12 gives panes their own shells, and a shell you cannot
/// create is a shell you cannot try. The real verbs — the pane head's menu, the
/// command palette — arrive with their own tickets, and they will call the same
/// [`Runtime::split_focused_terminal`] this chord calls.
///
/// The chord carries Alt for the reason the preview toggle does. `Ctrl+D` is a
/// real terminal control byte (EOT, "end of input") and `Ctrl+Shift+D` is short
/// enough that product will want it; a placeholder does not get to spend either.
/// Matched on the character the layout produced, and required to be the whole
/// chord and nothing looser.
fn split_shortcut_direction(key: &Key, modifiers: ModifiersState) -> Option<Axis> {
    if modifiers != ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SHIFT {
        return None;
    }
    match key {
        // Beside: a new column in the row this pane is in.
        Key::Character(text) if text.eq_ignore_ascii_case("d") => Some(Axis::Row),
        // Under: a new row in the column this pane is in.
        Key::Character(text) if text.eq_ignore_ascii_case("e") => Some(Axis::Col),
        _ => None,
    }
}

/// Undo close — "that one, now", and the door with the real traffic (N143).
///
/// The exact-equality test on the modifiers is the same discipline the preview
/// toggle uses and for the same reason: a bare `Ctrl+T` is a real control byte
/// and must keep reaching the child. Ctrl+**Shift**+T is not, which is why the
/// whole ecosystem could agree on it.
fn is_reopen_closed_tab_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers == ModifiersState::CONTROL | ModifiersState::SHIFT
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("t"))
}

/// Solve the tree against the current surface, and pick out the terminal seat.
///
/// The whole §4.1 conversion lives here and nowhere else: physical pixels in,
/// logical viewport, one `solve`, seat rectangles out. The solver is pure and
/// `O(seats)`, so calling it on every resize and every frame of a divider drag
/// is the preferred implementation rather than something to optimise away
/// (§4.4) — one geometry beats a cached one.
fn solve_seats(
    seats: &seats::Seats,
    renderer: &Renderer,
    render_physical: PhysicalSize<u32>,
) -> (
    SeatLayout,
    Option<seats::FitOverflow>,
    SeatViewport,
    LogicalRect,
) {
    let dpi_milli = renderer.metrics().dpi_milli().get();
    let metrics = seats::seat_metrics(dpi_milli);
    let viewport = seats::logical_viewport(
        render_physical.width,
        render_physical.height,
        seats::scale_ppm(dpi_milli),
    );
    let (layout, overflow) = match seats.solve(viewport, &metrics) {
        Ok(layout) => (layout, None),
        Err(_) => seats::fit_what_fits(seats, viewport, &metrics),
    };
    let terminal = seats::pane_body_viewport(
        seats,
        &layout,
        seats.terminal(),
        renderer.metrics().scale_factor as f32,
    )
    .unwrap_or(SeatViewport::whole(
        render_physical.width.max(1),
        render_physical.height.max(1),
    ));
    // The viewport travels out beside the layout it produced, so a drop plan
    // measured against that layout is measured against the very rectangle it was
    // solved into rather than against one recomputed from the same inputs and
    // hoped to agree (A12, T228).
    (layout, overflow, terminal, viewport)
}

/// A caret rectangle the frame produced, moved from the terminal seat's
/// coordinates into the window's — what winit and IMM32 are asking for.
///
/// The identity for a lone leaf is the point: its seat origin is `(0, 0)`, so
/// this is the number that was passed before seats existed.
fn window_ime_cursor_area(seat: SeatViewport, area: ImeCursorArea) -> ImeCursorArea {
    ImeCursorArea {
        x: area.x.saturating_add_unsigned(seat.x),
        y: area.y.saturating_add_unsigned(seat.y),
        ..area
    }
}

/// The pixel size ConPTY is told about: the terminal *seat's*, not the window's.
/// They are the same number for a lone leaf.
fn terminal_pty_physical(renderer: &Renderer, window: PhysicalSize<u32>) -> PhysicalSize<u32> {
    let seat = renderer.seat_viewport();
    if seat.width == 0 || seat.height == 0 {
        return window;
    }
    PhysicalSize::new(seat.width, seat.height)
}

/// What the session file asks the next window to open as.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RestoredPlacement {
    /// Logical size to reopen at. Always honoured, because a size is never
    /// off-screen.
    size: LogicalSize<f64>,
    /// Logical top-left, or `None` when no monitor this machine currently has
    /// can see the recorded rectangle and the OS should choose the spot instead.
    position: Option<LogicalPosition<f64>>,
    maximized: bool,
}

/// Where a restored window should open, or `None` on a first run.
///
/// docs/M2-persistence-schema-v1.md §3.1: hitting the recorded monitor and the
/// recorded logical coordinates is best effort, but "does not crash, does not
/// land off-screen" is the hard floor. So a rectangle that no monitor can see
/// forfeits its position — the size is still honoured, because a size is never
/// off-screen.
fn restore_window_placement(
    event_loop: &ActiveEventLoop,
    session: &SessionV1,
) -> Option<RestoredPlacement> {
    // An empty tab list is the first run: there is nothing to restore, and the
    // product's opening size stands.
    if session.tabs.is_empty() {
        return None;
    }
    let bounds = session.window.bounds;
    let size = LogicalSize::new(f64::from(bounds.width), f64::from(bounds.height));
    let position = LogicalPosition::new(f64::from(bounds.x), f64::from(bounds.y));
    let visible = event_loop.available_monitors().any(|monitor| {
        let scale = monitor.scale_factor().max(f64::MIN_POSITIVE);
        let origin = monitor.position();
        let extent = monitor.size();
        let left = f64::from(origin.x) / scale;
        let top = f64::from(origin.y) / scale;
        let right = left + f64::from(extent.width) / scale;
        let bottom = top + f64::from(extent.height) / scale;
        // "Some of the window is reachable" rather than "all of it fits": a
        // window whose title bar is on screen can always be dragged the rest of
        // the way, and demanding containment would move windows the user
        // deliberately parked half off a monitor.
        position.x < right
            && position.y < bottom
            && position.x + size.width > left
            && position.y + size.height > top
    });
    Some(RestoredPlacement {
        size,
        position: Some(position).filter(|_| visible),
        maximized: session.window.maximized,
    })
}

/// The physical outer rectangle a window must be given at startup.
///
/// Both halves of the round trip name the same rectangle — the window's *outer*
/// rect, the one `GetWindowRect` reports — so restoring is nothing but the
/// logical-to-physical direction of [`persisted_window_bounds`]. `opened_at`
/// supplies the corner for the two cases that have no recorded one: a first run,
/// and a rectangle whose monitor is gone.
fn startup_window_rect(
    placement: Option<RestoredPlacement>,
    opened_at: bt_platform::WindowRect,
    scale: f64,
) -> bt_platform::WindowRect {
    let scale = scale.max(f64::MIN_POSITIVE);
    let size = placement
        .map(|placement| placement.size)
        .unwrap_or(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT));
    let (left, top) = placement
        .and_then(|placement| placement.position)
        .map(|position| {
            (
                physical_px(position.x, scale),
                physical_px(position.y, scale),
            )
        })
        .unwrap_or((opened_at.left, opened_at.top));
    bt_platform::WindowRect {
        left,
        top,
        right: left.saturating_add(physical_px(size.width, scale).max(1)),
        bottom: top.saturating_add(physical_px(size.height, scale).max(1)),
    }
}

/// One physical-pixel coordinate from one logical one, saturating rather than
/// wrapping on the absurd values a hand-edited session file can hold.
fn physical_px(logical: f64, scale: f64) -> i32 {
    (logical * scale)
        .round()
        .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32
}

/// The logical rectangle the session file records, from the physical outer rect
/// Win32 reports. The inverse of [`startup_window_rect`]'s scaling: for every
/// scale factor Windows can report (never below 1.0), rounding out to physical
/// and back in lands on the same logical numbers, so a window nobody touched is
/// written back byte for byte on every restart.
fn persisted_window_bounds(rect: bt_platform::WindowRect, scale: f64) -> WindowBoundsV1 {
    let scale = scale.max(f64::MIN_POSITIVE);
    WindowBoundsV1 {
        x: (f64::from(rect.left) / scale).round() as i32,
        y: (f64::from(rect.top) / scale).round() as i32,
        width: (f64::from(rect.right.saturating_sub(rect.left)) / scale)
            .round()
            .max(1.0) as u32,
        height: (f64::from(rect.bottom.saturating_sub(rect.top)) / scale)
            .round()
            .max(1.0) as u32,
    }
}

fn pty_size(grid: GridSize, physical: PhysicalSize<u32>) -> PtySize {
    PtySize {
        columns: grid.columns,
        rows: grid.rows,
        pixel_width: physical.width.min(u32::from(u16::MAX)) as u16,
        pixel_height: physical.height.min(u32::from(u16::MAX)) as u16,
    }
}

fn load_probe_input() -> Result<Option<Vec<u8>>> {
    let Some(path) = std::env::var_os("BT_PROBE_INPUT") else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    std::fs::read(&path)
        .with_context(|| format!("read BT_PROBE_INPUT {}", path.display()))
        .map(Some)
}

fn install_panic_log_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("unnamed");
        let report = format!(
            "unix_ms={timestamp_ms} thread={thread_name}\npanic: {info}\nbacktrace:\n{}\n",
            Backtrace::force_capture()
        );
        let path = panic_log_path();
        if let Err(error) = append_panic_report(&path, &report) {
            eprintln!("failed to write panic report {}: {error}", path.display());
        }
        previous(info);
    }));
}

fn panic_log_path() -> PathBuf {
    std::env::temp_dir().join(PANIC_LOG_FILENAME)
}

fn append_panic_report(path: &std::path::Path, report: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{report}")
}

fn main() -> Result<()> {
    install_panic_log_hook();
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .context("create winit event loop")?;
    let mut application = BetterTerminalApp::new(event_loop.create_proxy());
    event_loop
        .run_app(&mut application)
        .map_err(|error| anyhow!(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_render::{DARK_CHROME, LIGHT_CHROME};
    use std::time::Duration;

    // ── T4: the press promise (J105) and the tab-name editor (J99-J104) ──

    const A: TabId = TabId(1);
    const B: TabId = TabId(2);

    fn at(x: f64, y: f64) -> PhysicalPosition<f64> {
        PhysicalPosition::new(x, y)
    }

    /// J105 (mock-up 5743-5765) — a press chooses a tab; the switch lands 180ms
    /// later.
    ///
    /// Red gate: `chrome_mouse_input` activated on the press itself
    /// (`ChromeTarget::Tab(index) => self.activate_tab(index, false)`), so there
    /// was no interval in which the tab was chosen but not yet shown — which is
    /// the entire mechanism. The three assertions are the three instants: at the
    /// press, one tick short of the deadline, and at it.
    #[test]
    fn a_press_chooses_a_tab_and_the_view_follows_a_hundred_and_eighty_milliseconds_later() {
        let now = Instant::now();
        let mut press = TabPress::armed(A, at(100.0, 20.0), now);

        assert_eq!(press.promise, TabPressPromise::Pending, "chosen, not shown");
        assert!(
            !press.matured(now + Duration::from_millis(179)),
            "179ms is still inside the grace period"
        );
        assert!(
            press.matured(now + TAB_PRESS_ACTIVATION_GRACE),
            "at 180ms the press has waited long enough to be believed"
        );
        assert_eq!(press.promise, TabPressPromise::Paid);
        assert!(
            !press.matured(now + Duration::from_secs(1)),
            "and it is paid once, not once per wake-up"
        );
    }

    /// J105's other half — "松开时不足 180ms → 立即激活(点击就是点击)".
    ///
    /// The release pays on the tab it pressed and nowhere else, which is the
    /// mock-up's `click` handler (5735) stated in geometry: `click` fires on the
    /// element the press and the release share.
    #[test]
    fn letting_go_on_the_tab_you_pressed_is_a_click_and_shows_it_at_once() {
        let now = Instant::now();

        let mut quick = TabPress::armed(A, at(100.0, 20.0), now);
        assert!(
            quick.released_over(Some(A)),
            "a click well inside the grace period is still a click"
        );
        assert_eq!(quick.promise, TabPressPromise::Paid);

        let mut elsewhere = TabPress::armed(A, at(100.0, 20.0), now);
        assert!(
            !elsewhere.released_over(Some(B)),
            "lifting on a different tab is not a click on this one"
        );
        assert!(
            !elsewhere.released_over(None),
            "and lifting off the strip is not a click at all"
        );

        let mut already = TabPress::armed(A, at(100.0, 20.0), now);
        assert!(already.matured(now + TAB_PRESS_ACTIVATION_GRACE));
        assert!(
            !already.released_over(Some(A)),
            "a promise is paid once — the release must not switch a second time"
        );
    }

    /// J105 — "位移超过拖拽阈值(6px)之前,不切换内容 ... 快速拖走时不会闪一下".
    ///
    /// The threshold is `startDrag`'s own 6 logical pixels (mock-up 6727) and it
    /// is *logical*: the same hand movement on a 200% display crosses twice the
    /// physical pixels and is the same movement.
    #[test]
    fn travelling_past_six_pixels_abandons_the_delayed_switch() {
        let now = Instant::now();
        for scale in [1.0, 1.5, 2.0] {
            let mut press = TabPress::armed(A, at(100.0, 20.0), now);
            assert!(
                !press.travelled(at(100.0 + 5.9 * scale, 20.0), scale),
                "just under the threshold at {scale}x is still a press"
            );
            assert_eq!(press.promise, TabPressPromise::Pending);
            assert!(
                press.travelled(at(100.0 + 6.0 * scale, 20.0), scale),
                "at the threshold the press becomes a drag at {scale}x"
            );
            assert_eq!(
                press.promise,
                TabPressPromise::Slipped,
                "the state T5 hangs its drag on"
            );
            assert!(
                !press.matured(now + TAB_PRESS_ACTIVATION_GRACE),
                "and the timer that is still running must find nothing to do"
            );
            assert_eq!(press.wake_deadline(), None, "nor ask to be woken for it");
        }
    }

    /// J105/J108 — the promise a slipped press still carries.
    ///
    /// Two facts T5 needs and this slice must not get wrong: travelling after
    /// the switch has already landed does not take it back, and a slipped press
    /// that comes home still pays (the mock-up's release path re-selects on
    /// exactly this condition, 6888 and 7134).
    #[test]
    fn a_slipped_press_keeps_its_promise_and_a_paid_one_cannot_be_unpaid() {
        let now = Instant::now();

        let mut after_paying = TabPress::armed(A, at(100.0, 20.0), now);
        assert!(after_paying.matured(now + TAB_PRESS_ACTIVATION_GRACE));
        assert!(
            after_paying.travelled(at(400.0, 20.0), 1.0),
            "a finger held past the grace period is still a finger that can carry the tab"
        );
        assert_eq!(
            after_paying.promise,
            TabPressPromise::Paid,
            "dragging a tab you are already looking at does not un-choose it"
        );

        let mut came_home = TabPress::armed(A, at(100.0, 20.0), now);
        assert!(came_home.travelled(at(120.0, 20.0), 1.0));
        assert!(
            came_home.released_over(Some(A)),
            "down and up on one tab is a click however the pointer wandered between"
        );
        assert!(
            !TabPress::armed(A, at(100.0, 20.0), now).released_over(Some(B)),
            "and the promise stays unpaid when the release lands elsewhere"
        );
    }

    /// J105 — "在已激活的 tab 上按下" owes nothing, and must not be able to
    /// start owing something later (mock-up 5755: `wsId !== state.active`).
    #[test]
    fn pressing_the_tab_you_are_already_on_promises_nothing() {
        let now = Instant::now();
        let mut press = TabPress::settled(A, at(100.0, 20.0), now);
        assert_eq!(press.promise, TabPressPromise::Paid);
        assert_eq!(press.wake_deadline(), None, "no timer is armed");
        assert!(!press.matured(now + TAB_PRESS_ACTIVATION_GRACE));
        assert!(!press.released_over(Some(A)));
    }

    /// The bug T5 shipped with, off real hardware: with two tabs open, the tab
    /// you were looking at could not be dragged *at all*. It read as
    /// "sometimes" because which tab that is changes as you work — and the tab
    /// a hand reaches for is very often the one already in front of it.
    ///
    /// Red gate: `travelled` refused any press whose promise was not `Pending`,
    /// and a press onto the active tab is born `Paid` (`TabPress::settled`), so
    /// it never reported the move a drag begins on, at any distance.
    #[test]
    fn the_tab_you_are_already_on_is_still_draggable() {
        let now = Instant::now();
        for scale in [1.0, 1.5, 2.0] {
            let mut press = TabPress::settled(A, at(100.0, 20.0), now);
            assert!(
                !press.travelled(at(100.0 + 5.9 * scale, 20.0), scale),
                "the active tab is held to the same 6px as every other tab"
            );
            assert!(
                press.travelled(at(100.0 + 7.0 * scale, 20.0), scale),
                "and past it the press is a drag at {scale}x, owing nothing or not"
            );
            assert_eq!(
                press.promise,
                TabPressPromise::Paid,
                "the drag neither borrows from the promise nor pays it back"
            );
            assert!(
                !press.travelled(at(400.0, 20.0), scale),
                "and it still starts exactly once"
            );
        }
    }

    /// J99/K111 — the second press of a would-be rename lands on the tab the
    /// first click just activated, so it too owes nothing. Holding it and
    /// moving carries the tab; it does not sit inside the double-click window
    /// waiting for an editor.
    #[test]
    fn pressing_again_inside_the_double_click_window_and_moving_is_a_drag() {
        let now = Instant::now();
        let mut clicks = TabClicks::default();

        let mut first = TabPress::armed(A, at(100.0, 20.0), now);
        assert!(first.released_over(Some(A)), "click one shows the tab");
        assert_eq!(clicks.register(A, now), TabClick::Single);

        // Press two, well inside the window — and now onto the active tab.
        let again = now + Duration::from_millis(80);
        let mut second = TabPress::settled(A, at(100.0, 20.0), again);
        assert!(
            second.travelled(at(106.0, 20.0), 1.0),
            "a press-and-move inside the double-click window is a drag, not half a rename"
        );

        // And the gesture takes its own release with it: `drop_tab_drag`
        // interrupts the chain, so the lift that ends the drag cannot land as
        // the second click and open the editor behind the drop.
        clicks.interrupt();
        assert_eq!(
            clicks.register(A, again + Duration::from_millis(10)),
            TabClick::Single,
            "a click that became a drag is not the first half of anything"
        );
    }

    /// J99/J105 — the two clicks of a rename land on one tab inside the
    /// system's own double-click window, and nothing else pairs with them.
    #[test]
    fn two_presses_on_one_tab_inside_the_double_click_window_are_a_double_click() {
        let now = Instant::now();
        let mut clicks = TabClicks::default();

        assert_eq!(clicks.register(A, now), TabClick::Single);
        assert_eq!(
            clicks.register(A, now + MULTI_CLICK_INTERVAL),
            TabClick::Double,
            "the far edge of the window still pairs"
        );
        assert_eq!(
            clicks.register(A, now + MULTI_CLICK_INTERVAL + Duration::from_millis(1)),
            TabClick::Single,
            "a double click consumes its own history — a third press starts over"
        );

        let mut slow = TabClicks::default();
        assert_eq!(slow.register(A, now), TabClick::Single);
        assert_eq!(
            slow.register(A, now + MULTI_CLICK_INTERVAL + Duration::from_millis(1)),
            TabClick::Single,
            "past the window it is two single clicks"
        );

        let mut wandering = TabClicks::default();
        assert_eq!(wandering.register(A, now), TabClick::Single);
        assert_eq!(
            wandering.register(B, now + Duration::from_millis(10)),
            TabClick::Single,
            "two tabs are two elements, and `dblclick` needs one"
        );

        // J99: "`.close`/`.pin` 上的双击不算(那是两次按钮点击)" — the button
        // press never registers, and it breaks the chain on its way past.
        let mut interrupted = TabClicks::default();
        assert_eq!(interrupted.register(A, now), TabClick::Single);
        interrupted.interrupt();
        assert_eq!(
            interrupted.register(A, now + Duration::from_millis(10)),
            TabClick::Single,
            "a click on the × between them is not the first half of anything"
        );
    }

    /// J101 (mock-up 5863-5870) — the box opens holding YOUR name and nothing
    /// else, with all of it selected and the caret at its end.
    #[test]
    fn the_editor_opens_holding_only_the_name_you_typed() {
        let named = TabRename::open(A, Some("build"));
        assert_eq!(named.text, "build");
        assert_eq!(
            named.caret, 5,
            "`input.select()` leaves the caret at the end"
        );
        assert!(named.select_all, "and the whole of it selected");

        // The auto name is never *in* the box — it is behind it. A tab that has
        // never been named opens empty, which is what makes the placeholder the
        // only thing you can see.
        let unnamed = TabRename::open(A, None);
        assert_eq!(unnamed.text, "");
        assert_eq!(unnamed.caret, 0);
        assert!(
            !unnamed.select_all,
            "there is nothing to select, so nothing is"
        );
    }

    /// J101/J102 — typing over the opening selection replaces it, and the
    /// draft's own verbs move on character boundaries rather than byte ones.
    #[test]
    fn typing_over_the_opening_selection_replaces_the_whole_name() {
        let mut editor = TabRename::open(A, Some("build"));
        editor.insert("x");
        assert_eq!(
            editor.text, "x",
            "the selection went with the first keystroke"
        );
        assert_eq!(editor.caret, 1);
        assert!(!editor.select_all);

        // Backspace on a fresh selection clears it rather than eating one letter.
        let mut cleared = TabRename::open(A, Some("build"));
        cleared.backspace();
        assert_eq!(cleared.text, "");
        assert_eq!(cleared.caret, 0);

        // Arrow keys collapse to the near edge, so an accidental select-all is
        // recoverable rather than destructive.
        let mut left = TabRename::open(A, Some("build"));
        left.move_left();
        assert_eq!((left.text.as_str(), left.caret), ("build", 0));
        let mut right = TabRename::open(A, Some("build"));
        right.move_right();
        assert_eq!((right.text.as_str(), right.caret), ("build", 5));
    }

    /// J102/§7.1.5 — the editor's minimum verb set, over text that is not ASCII.
    ///
    /// A tab name is exactly the kind of short label that gets typed in Chinese,
    /// and a caret that counts bytes would land inside a character and panic on
    /// the next slice.
    #[test]
    fn the_editors_verbs_move_by_character_and_not_by_byte() {
        let mut editor = TabRename::open(A, Some("构建"));
        editor.move_left();
        assert_eq!(editor.caret, 0, "collapse to the near edge first");
        editor.move_right();
        assert_eq!(editor.caret, 3, "one three-byte character");
        editor.insert("x");
        assert_eq!(editor.text, "构x建");
        editor.backspace();
        assert_eq!(editor.text, "构建");
        assert_eq!(editor.caret, 3);
        editor.delete();
        assert_eq!(editor.text, "构", "Delete takes the character in front");
        editor.move_home();
        assert_eq!(editor.caret, 0);
        editor.delete();
        assert_eq!(editor.text, "");
        editor.backspace();
        assert_eq!(editor.text, "", "and an empty draft survives a backspace");
        editor.move_end();
        assert_eq!(editor.caret, 0);
    }

    /// J102 (mock-up 5883) — "空串 = 撤销 override(name=null)".
    ///
    /// The sanitiser runs before the emptiness test, so a draft of nothing but
    /// spaces is the same answer as a draft of nothing: a name of no characters
    /// is the absence of a name, not a name that is blank.
    #[test]
    fn an_empty_draft_drops_the_override_rather_than_naming_the_tab_nothing() {
        let mut editor = TabRename::open(A, Some("build"));
        editor.backspace();
        assert_eq!(editor.committed_name(), None, "the override is dropped");

        let mut spaces = TabRename::open(A, None);
        spaces.insert("   ");
        assert_eq!(
            spaces.committed_name(),
            None,
            "trimmed to nothing is nothing"
        );

        let mut named = TabRename::open(A, None);
        named.insert("  build  ");
        assert_eq!(
            named.committed_name().as_deref(),
            Some("build"),
            "and a real name arrives sanitised, exactly as every other layer does"
        );

        let mut long = TabRename::open(A, None);
        long.insert(&"n".repeat(60));
        assert_eq!(
            long.committed_name().map(|name| name.chars().count()),
            Some(TITLE_MAX_CHARS),
            "the cap the other three layers already answer to"
        );
    }

    /// J103 — "编辑期间键盘输入不进终端(编辑器独占)".
    ///
    /// Every arm returns a verdict and there is no arm that hands the key on.
    /// The two that leave are the two the mock-up has: Enter commits, Escape
    /// abandons (5895-5896).
    #[test]
    fn the_open_editor_owns_the_keyboard_and_gives_back_only_two_keys() {
        let none = ModifiersState::empty();
        let mut editor = TabRename::open(A, None);

        assert_eq!(
            rename_key(&mut editor, &Key::Character("a".into()), none),
            RenameVerdict::Held
        );
        assert_eq!(editor.text, "a");
        assert_eq!(
            rename_key(&mut editor, &Key::Named(NamedKey::Space), none),
            RenameVerdict::Held
        );
        assert_eq!(editor.text, "a ", "space is text, not a verb");

        // A chord has no meaning here and must not fall through to the shell:
        // Ctrl+C in a name box is not an interrupt, it is nothing.
        assert_eq!(
            rename_key(
                &mut editor,
                &Key::Character("c".into()),
                ModifiersState::CONTROL
            ),
            RenameVerdict::Held
        );
        assert_eq!(editor.text, "a ", "and it typed nothing either");

        assert_eq!(
            rename_key(&mut editor, &Key::Named(NamedKey::F5), none),
            RenameVerdict::Held,
            "a key with no verb is swallowed, not passed to the terminal"
        );

        assert_eq!(
            rename_key(&mut editor, &Key::Named(NamedKey::Enter), none),
            RenameVerdict::Commit
        );
        assert_eq!(
            rename_key(&mut editor, &Key::Named(NamedKey::Escape), none),
            RenameVerdict::Cancel
        );
    }

    // ── T3: one vault, three doors ──

    fn saved_tab(profile_id: &str, cwd: &str, name: Option<&str>, pinned: bool) -> TabV1 {
        TabV1 {
            root: LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                profile_id: profile_id.to_owned(),
                cwd: cwd.to_owned(),
                manual_name: name.map(str::to_owned),
            })),
            pinned,
            focused_leaf: "leaf-0".to_owned(),
        }
    }

    /// PIN — mock-up 7426-7431: "Launch asks about exactly one thing, and it is
    /// not the pinned tabs. **Pinning IS the answer**."
    ///
    /// Red gate: `Runtime::create` used to rebuild *every* persisted tab
    /// unconditionally, which is both halves of this wrong at once — it asked
    /// nothing, and it restored what the user may well have meant to close.
    #[test]
    fn launch_opens_what_you_pinned_and_asks_only_about_the_rest() {
        let saved = [
            saved_tab("pwsh", "C:\\a", None, false),
            saved_tab("pwsh", "C:\\b", None, true),
            saved_tab("pwsh", "C:\\c", None, false),
        ];
        let plan = plan_launch(&saved, 0);

        assert_eq!(plan.open, vec![saved[1].clone()], "the pinned one, alone");
        assert_eq!(
            plan.ask,
            vec![saved[0].clone(), saved[2].clone()],
            "the question is the tabs you did not pin, in their own order"
        );
        assert!(
            !plan.placeholder,
            "a pinned tab is already a window worth showing"
        );
        assert_eq!(
            plan.active_open, None,
            "the tab you were on was not pinned, so it is not one of these"
        );
    }

    /// The seat you were in comes back with you — but only if it was pinned.
    #[test]
    fn the_tab_you_were_on_keeps_its_seat_when_it_is_one_of_the_pinned() {
        let saved = [
            saved_tab("pwsh", "C:\\a", None, false),
            saved_tab("pwsh", "C:\\b", None, true),
            saved_tab("pwsh", "C:\\c", None, true),
        ];
        // index 2 of the saved list is the second *pinned* tab.
        assert_eq!(plan_launch(&saved, 2).active_open, Some(1));
        assert_eq!(plan_launch(&saved, 1).active_open, Some(0));
    }

    /// The boundary ruled in this ticket: "Reopen your **other** tabs?" needs
    /// other tabs. With one unpinned tab and nothing pinned there is no question
    /// — and declining would have handed back a fresh shell in the wrong folder,
    /// which is strictly worse than the tab it replaced.
    #[test]
    fn a_lone_unpinned_tab_is_restored_rather_than_asked_about() {
        let saved = [saved_tab("pwsh", "C:\\only", None, false)];
        let plan = plan_launch(&saved, 0);

        assert_eq!(plan.open, saved.to_vec(), "it simply comes back");
        assert!(plan.ask.is_empty(), "nothing to ask");
        assert!(!plan.placeholder, "it is a real tab, not scaffolding");
        assert_eq!(plan.active_open, Some(0));

        // Two unpinned tabs *are* a question, and then a stand-in shell carries
        // the window until it is answered.
        let two = [
            saved_tab("pwsh", "C:\\a", None, false),
            saved_tab("pwsh", "C:\\b", None, false),
        ];
        let plan = plan_launch(&two, 0);
        assert!(plan.open.is_empty());
        assert_eq!(plan.ask.len(), 2);
        assert!(
            plan.placeholder,
            "nothing was pinned, so nothing is standing"
        );
    }

    #[test]
    fn a_first_launch_with_nothing_saved_asks_nothing_and_stands_something_up() {
        let plan = plan_launch(&[], 0);
        assert!(plan.open.is_empty());
        assert!(plan.ask.is_empty(), "no prompt on a first run");
        assert!(plan.placeholder);
    }

    /// A tab's identity is the terminal it holds, wherever that sits in the tree.
    #[test]
    fn a_seed_is_read_from_the_first_terminal_in_the_tree() {
        let split = LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 500_000,
            children: [
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Files(
                    bt_persist::FilesLeafV1 {
                        root: "C:\\repo".to_owned(),
                        open: Vec::new(),
                        sel: None,
                        width: 240,
                    },
                ))),
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                    profile_id: "pwsh".to_owned(),
                    cwd: "C:\\repo\\src".to_owned(),
                    manual_name: Some("build".to_owned()),
                }))),
            ],
        });
        let leaf = first_term_leaf(&split).expect("a files pane is not the tab's identity");
        assert_eq!(leaf.cwd, "C:\\repo\\src");
        assert_eq!(leaf.manual_name.as_deref(), Some("build"));

        // A files-only tree has no terminal to speak for it.
        assert!(
            first_term_leaf(&LayoutNodeV1::Leaf(LeafNodeV1::Unknown)).is_none(),
            "an unknown leaf is not a terminal"
        );
    }

    /// §5.4 逐叶降级, "未知 profile→默认": a profile this build does not have
    /// costs you the shell choice, never the tab.
    #[test]
    fn a_seed_naming_a_profile_we_do_not_have_still_comes_back() {
        let (_, seed, _) = revive_plan(&saved_tab("wsl-ubuntu", "C:\\a", Some("notes"), true));
        assert_eq!(seed.profile, profiles::DEFAULT_PROFILE);
        assert_eq!(
            seed.manual_name.as_deref(),
            Some("notes"),
            "your name stays"
        );
        assert!(seed.pinned, "and so does the promise");
    }

    /// N143. Exact-equality on the modifiers, for the same reason the preview
    /// toggle uses it: a bare `Ctrl+T` is a real control byte and has to keep
    /// reaching the child.
    #[test]
    fn undo_close_answers_to_ctrl_shift_t_and_to_nothing_looser() {
        let lower = Key::Character("t".into());
        let upper = Key::Character("T".into());
        let chord = ModifiersState::CONTROL | ModifiersState::SHIFT;

        assert!(is_reopen_closed_tab_shortcut(&lower, chord));
        assert!(
            is_reopen_closed_tab_shortcut(&upper, chord),
            "Shift is in the chord, so the character arrives capitalised"
        );
        assert!(
            !is_reopen_closed_tab_shortcut(&lower, ModifiersState::CONTROL),
            "bare Ctrl+T is the child's"
        );
        assert!(!is_reopen_closed_tab_shortcut(
            &lower,
            ModifiersState::empty()
        ));
        assert!(
            !is_reopen_closed_tab_shortcut(&lower, chord | ModifiersState::ALT),
            "a longer chord is a different chord"
        );
        assert!(!is_reopen_closed_tab_shortcut(
            &Key::Character("p".into()),
            chord
        ));
    }

    // ── T2: the tab strip's state channels ──

    fn facts(status: SessionStatus, last_seen_revision: u64, tab_is_active: bool) -> SessionFacts {
        SessionFacts {
            status,
            last_seen_revision,
            tab_is_active,
        }
    }

    /// A session that has published `revision` frames and is otherwise quiet.
    fn quiet(revision: u64) -> SessionStatus {
        SessionStatus {
            progress: None,
            bell_latched: false,
            failure_exit_code: None,
            working: false,
            published_revision: revision,
        }
    }

    /// PIN (T2 J97): the tab on screen keeps its ledger current, so leaving it
    /// does not retroactively invent unread output.
    ///
    /// Red gate, and the subtlest bug in this whole片. Suppressing the dot on
    /// the active tab is only half the rule — it hides the claim without
    /// answering it. If the ledger itself stops advancing while the tab is
    /// watched, then every frame the user sat and read piles up behind it, and
    /// the moment they switch away the tab they *just left* lights up claiming
    /// to hold output they had been staring at. Watching is seeing, and this is
    /// where that gets written down.
    #[test]
    fn the_watched_tab_keeps_its_ledger_current() {
        // Watched: the ledger tracks publication as it happens.
        assert_eq!(seen_revision(3, 90, true), 90);
        // Unwatched: it holds still, which is what makes new output count.
        assert_eq!(seen_revision(3, 90, false), 3);

        // The whole sequence the bug lives in: open a tab, read a while, leave.
        let mut seen = seen_revision(0, 0, true);
        for published in 1..=40 {
            seen = seen_revision(seen, published, true);
        }
        let facts = SessionFacts {
            status: quiet(40),
            last_seen_revision: seen,
            tab_is_active: false,
        };
        assert!(
            !facts.has_unseen_output(),
            "a tab just switched away from holds nothing unread"
        );
        // And the very next thing it publishes does count.
        let facts = SessionFacts {
            status: quiet(41),
            last_seen_revision: seen,
            tab_is_active: false,
        };
        assert!(
            facts.has_unseen_output(),
            "output after the switch is unread"
        );
    }

    /// A session whose bell has rung and whose last command failed — the two
    /// latches [`DualPlaneSession::clear_attention`] retires together.
    fn latched(revision: u64) -> SessionStatus {
        SessionStatus {
            bell_latched: true,
            failure_exit_code: Some(1),
            ..quiet(revision)
        }
    }

    /// Apply the ledger's attention rule the way the runtime's own loop does,
    /// and report what the tab ends up claiming.
    ///
    /// The clearing is `clear_attention`'s two field writes, which is all the
    /// runtime does with the answer — so this exercises the real decision
    /// rather than a second copy of it.
    fn claim_after_a_look(
        mut status: SessionStatus,
        tab_is_active: bool,
        window_is_focused: bool,
    ) -> StatusClaim {
        if attention_is_consumed(tab_is_active, window_is_focused) {
            status.bell_latched = false;
            status.failure_exit_code = None;
        }
        SessionFacts {
            status,
            // Behind by a mile, so nothing here is hidden by the ledger being
            // up to date: whatever survives is the attention rule's own doing.
            last_seen_revision: 0,
            tab_is_active,
        }
        .claim()
    }

    /// PIN (T2, user ruling — "watching is consuming"): a latch that arrives on
    /// the tab the user is already reading is spent on arrival.
    ///
    /// The bell is the case that matters, because it is the one claim the
    /// work-in-flight rule never suppresses: without this it would sit on the
    /// focused tab indefinitely, since nothing else in the taxonomy retires a
    /// latch except switching away and back. A terminal you are looking at does
    /// not need a dot repeating what it just showed you.
    #[test]
    fn a_latch_arriving_on_the_watched_tab_is_spent_on_arrival() {
        assert!(attention_is_consumed(true, true));
        assert_eq!(
            claim_after_a_look(latched(50), true, true),
            StatusClaim::Silent,
            "the tab in front of the user wears no dot"
        );
        // Both latches go, not just the loud one — a failure read on screen is
        // as read as a bell heard on screen.
        let mut failed_only = quiet(50);
        failed_only.failure_exit_code = Some(1);
        assert_eq!(
            claim_after_a_look(failed_only, true, true),
            StatusClaim::Silent
        );
    }

    /// PIN (T2, user ruling): an unfocused window consumes nothing.
    ///
    /// This is the half that makes the rule safe. The active tab is still the
    /// active tab when the user alt-tabs away, so clearing on "active" alone
    /// would eat every bell that rang while they were gone — which is precisely
    /// the moment a bell is doing its job. Nobody is reading a background
    /// window, so nothing in it is read.
    #[test]
    fn a_bell_that_rings_while_the_user_is_away_is_still_waiting() {
        assert!(!attention_is_consumed(true, false));
        // Both latches survive, but only the bell can *show* on the tab that is
        // on screen: a failure is a kind of unread, and unread is suppressed on
        // the active tab whatever the window is doing. So the bell is not
        // merely the loudest surviving claim here — it is the only one this
        // ruling can be about, which is why the ruling is about the bell.
        let mut bell_only = quiet(50);
        bell_only.bell_latched = true;
        for status in [latched(50), bell_only] {
            assert_eq!(
                claim_after_a_look(status, true, false),
                StatusClaim::Bell,
                "an unfocused window keeps its bell"
            );
        }
        // And a bell on the *active* tab really can show a dot at all — the
        // active-tab suppression covers unread and failure, never the bell.
        assert_eq!(
            SessionFacts {
                status: bell_only,
                last_seen_revision: 50,
                tab_is_active: true,
            }
            .claim(),
            StatusClaim::Bell
        );
    }

    /// PIN (T2, user ruling): a tab nobody is looking at is unchanged — its
    /// latches wait for the activation that answers them.
    ///
    /// The whole point of the taxonomy is the tabs you *cannot* see, so the
    /// rule above must not reach them under any combination of focus. Only
    /// `TabState::mark_seen`, on activation, retires these.
    #[test]
    fn an_inactive_tabs_latches_wait_for_the_activation_that_answers_them() {
        for window_is_focused in [true, false] {
            assert!(
                !attention_is_consumed(false, window_is_focused),
                "focus alone consumes nothing on a tab that is not on screen"
            );
            assert_eq!(
                claim_after_a_look(latched(50), false, window_is_focused),
                StatusClaim::Failed,
                "an unwatched tab keeps its claim whatever the window is doing"
            );
        }
        // The full truth table, so the rule cannot drift into a one-sided test:
        // it is an `and`, and each half is load-bearing on its own.
        assert!(attention_is_consumed(true, true));
        assert!(!attention_is_consumed(true, false));
        assert!(!attention_is_consumed(false, true));
        assert!(!attention_is_consumed(false, false));
    }

    /// The mark state of a tab whose session is running, sampled mid-breath.
    fn breathing(motion: Motion) -> seats::TabMarkState {
        let elapsed = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(0.5);
        seats::TabMarkState {
            opacity: mark_opacity(true, false, elapsed, motion),
            ..seats::TabMarkState::default()
        }
    }

    /// The same tab one instant after its command returned.
    fn settled(motion: Motion) -> seats::TabMarkState {
        let elapsed = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(0.5);
        seats::TabMarkState {
            opacity: mark_opacity(false, false, elapsed, motion),
            ..seats::TabMarkState::default()
        }
    }

    /// PIN (T2, real-machine bug): a session that stops working returns its mark
    /// to full opacity — at every phase of the breath, and under both motion
    /// preferences.
    ///
    /// Red gate, reproduced on hardware: after `Start-Sleep 8` returned, the
    /// tab icon sat at opacity **0.379** and stayed there. The breath is a
    /// function of elapsed time, so asking it where it was at the moment work
    /// stopped returns whatever the curve happened to be passing through —
    /// which is any value in `.28 ..= 1.0`, and almost never `1.0`. The answer
    /// cannot be interpolated; it has to be a rule, and this is that rule.
    #[test]
    fn a_session_that_stops_working_returns_its_mark_to_full_opacity() {
        let period = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS);
        for motion in [Motion::Full, Motion::Reduced] {
            for step in 0..=64 {
                let elapsed = period.mul_f32(step as f32 / 32.0);
                assert_eq!(
                    mark_opacity(false, false, elapsed, motion),
                    1.0,
                    "{motion:?}: a mark that is not working is never faded, \
                     whatever phase the breath had reached"
                );
                // A ring has replaced the mark, so the mark is not faded either
                // — the ring is already saying "still going" in its own medium.
                assert_eq!(mark_opacity(true, true, elapsed, motion), 1.0);
            }
            // And while it *is* working the mark really is faded, or the pin
            // above would pass on a build that had simply deleted the breath.
            assert!(breathing(motion).opacity < 1.0, "{motion:?}: it breathes");
        }
    }

    /// PIN (T2, real-machine bug): the frame on which motion *stops* is owed a
    /// redraw, and the tab asks for it without waiting for any later event.
    ///
    /// This is the bug's real seat. The scheduler used to ask "is anything
    /// moving?", which is the right question for how long to keep waking up and
    /// the wrong one for whether to draw — and the two part company at exactly
    /// one moment, the moment motion ends. Nothing was moving any more, so the
    /// old predicate said "nothing owed" and returned before rebuilding,
    /// leaving whatever half-transparent frame the breath ended on as the last
    /// thing ever drawn. Nothing else was coming: the command was over, so the
    /// terminal had gone quiet too.
    #[test]
    fn the_frame_on_which_the_breath_stops_is_owed_a_redraw() {
        let mid_breath = breathing(Motion::Full);
        let finished = settled(Motion::Full);
        assert_ne!(mid_breath, finished, "the two frames really do differ");
        assert!(
            tab_owes_frame(Some(mid_breath), finished),
            "the working -> idle transition must ask for the frame that \
             puts the mark back to full opacity"
        );
        // What that frame carries is the settled state, at full opacity.
        assert_eq!(finished.opacity, 1.0);
        // Having drawn it, the tab stops asking — the fix must not turn a
        // missing frame into an endless stream of them.
        assert!(
            !tab_owes_frame(Some(finished), finished),
            "a settled tab owes nothing further"
        );
        // A tab that has never drawn anything owes its first frame.
        assert!(tab_owes_frame(None, finished));
    }

    /// PIN (T2, real-machine bug — the symmetric paths): every other channel
    /// that can *stop* is owed the same final frame.
    ///
    /// The root cause was never specific to the breath: it was a scheduler that
    /// could not see a channel switching off. These are the three siblings that
    /// were broken by the same line, and each is checked here so a future
    /// "optimisation" back to an is-it-moving test fails loudly rather than
    /// silently freezing one channel at a time.
    #[test]
    fn every_channel_that_stops_is_owed_its_final_frame() {
        let palette = LIGHT_CHROME;

        // 1. An indeterminate ring clearing back to its mark. This one was
        //    doubly hidden: the old ring signal watched the sweep and the
        //    tween, and an indeterminate ring keeps neither.
        let spinning = seats::TabMarkState {
            ring: Some(seats::TabRing {
                arc: palette.accent,
                start_milliturns: 250,
                sweep_milliturns: 243,
            }),
            ..seats::TabMarkState::default()
        };
        let cleared = seats::TabMarkState::default();
        assert!(
            tab_owes_frame(Some(spinning), cleared),
            "a ring that clears must hand the slot back to the mark"
        );

        // 2. Reduced motion, where the mark never animates at all — it steps
        //    between two held values. An is-it-moving test is blind to *both*
        //    edges here, so the held .6 would never arrive and never leave.
        let held = breathing(Motion::Reduced);
        assert_eq!(held.opacity, WINDOW_TAB_BREATHE_REDUCED_OPACITY);
        let done = settled(Motion::Reduced);
        assert!(
            tab_owes_frame(Some(done), held),
            "reduced motion must still show that work has started"
        );
        assert!(tab_owes_frame(Some(held), done), "and that it has finished");

        // 3. A dot arriving on a tab that is not moving and never was — a bell
        //    on a background tab. Nothing animates, so nothing asked to draw.
        let quiet_tab = seats::TabMarkState::default();
        let ringing = seats::TabMarkState {
            dot: Some(palette.status_warn),
            ..seats::TabMarkState::default()
        };
        assert!(
            tab_owes_frame(Some(quiet_tab), ringing),
            "a bell on a still tab must still light its dot"
        );
    }

    /// PIN (T2 D41): the accessibility preference is read in the right
    /// direction.
    ///
    /// Win32 and CSS spell this setting with opposite polarity —
    /// `SPI_GETCLIENTAREAANIMATION` is `TRUE` when animation is *wanted*, while
    /// `prefers-reduced-motion: reduce` matches when it is *not* — and the
    /// inversion is invisible on any machine left at the default. Getting it
    /// backwards would force animation on exactly the users who asked for none
    /// and strip it from everyone else, and no screenshot review would catch
    /// it. So the mapping is a named function with a test rather than a `!` at
    /// a call site.
    #[test]
    fn the_reduced_motion_preference_is_read_in_the_right_direction() {
        assert_eq!(
            Motion::from_client_area_animation(Some(true)),
            Motion::Full,
            "TRUE means the system wants animation"
        );
        assert_eq!(
            Motion::from_client_area_animation(Some(false)),
            Motion::Reduced,
            "FALSE is the accessibility setting turned on"
        );
        assert_eq!(
            Motion::from_client_area_animation(None),
            Motion::Full,
            "a failed read is not a request for less motion"
        );
        // The default a `Motion` takes when nothing has asked is the same one a
        // failed read gets, so the two cannot drift apart.
        assert_eq!(Motion::default(), Motion::Full);
    }

    /// PIN (T2 D34): a tab wears the loudest claim any of its sessions makes,
    /// and the ladder is `fail > bell > unread`.
    ///
    /// Red gate, and the mock-up records it as a user correction (line 1930):
    /// "panes wore dots while the tab wore none". A tab is a lid over sessions
    /// the user cannot see, so a claim it fails to pass up is a claim that is
    /// simply lost.
    #[test]
    fn a_tab_wears_the_loudest_claim_of_its_sessions() {
        use StatusClaim::{Bell, Failed, Silent, Unread};
        assert_eq!(loudest_claim([Silent, Unread, Bell]), Bell);
        assert_eq!(loudest_claim([Unread, Failed, Bell]), Failed);
        assert_eq!(loudest_claim([Silent, Unread]), Unread);
        assert_eq!(loudest_claim([Silent, Silent]), Silent);
        // A tab with no sessions at all claims nothing rather than panicking.
        assert_eq!(loudest_claim([]), Silent);
        // The ladder itself, stated once so the `max` above cannot silently
        // reorder: every louder claim outranks every quieter one.
        assert!(Silent < Unread && Unread < Bell && Bell < Failed);
        // And a tab never says *less* than any one of its members.
        for members in [
            vec![Silent, Failed],
            vec![Bell, Unread, Silent],
            vec![Unread],
        ] {
            let tab = loudest_claim(members.iter().copied());
            for member in members {
                assert!(tab >= member, "a tab must never say less than its panes");
            }
        }
    }

    /// PIN (T2 D35): work in flight suppresses every "finished" claim.
    ///
    /// The mock-up's own comment is a user ruling (line 1920): "an active
    /// download is still WORK IN FLIGHT: no finished-unread claim until the
    /// progress ends". The ring and the breathing icon are already reporting
    /// what is happening, and a dot beside them would be a third voice on one
    /// fact — and a wrong one, since nothing has finished.
    #[test]
    fn a_session_still_working_makes_no_finished_claim() {
        let unseen = quiet(9);
        // Quiet and unseen: the plain unread claim.
        assert_eq!(facts(unseen, 4, false).claim(), StatusClaim::Unread);

        // The same session, still running.
        let mut working = unseen;
        working.working = true;
        assert_eq!(facts(working, 4, false).claim(), StatusClaim::Silent);

        // The same session, reporting progress — suppressed in every flavour,
        // because every one of them means a run that has not ended.
        for state in [
            ProgressState::Normal(40),
            ProgressState::Indeterminate,
            ProgressState::Paused(Some(40)),
            ProgressState::Error(Some(40)),
        ] {
            let mut in_flight = unseen;
            in_flight.progress = Some(state);
            assert_eq!(
                facts(in_flight, 4, false).claim(),
                StatusClaim::Silent,
                "{state:?} is work in flight, not a finished claim"
            );
        }

        // A failure is suppressed by the same rule, for the same reason.
        let mut failing = unseen;
        failing.failure_exit_code = Some(1);
        assert_eq!(facts(failing, 4, false).claim(), StatusClaim::Failed);
        failing.progress = Some(ProgressState::Normal(10));
        assert_eq!(facts(failing, 4, false).claim(), StatusClaim::Silent);
    }

    /// PIN (T2): the bell is latched, so it survives what suppresses the rest.
    ///
    /// A bell is a thing that *rang* — a past event, not a state — so a session
    /// that is busy again has still rung, and the claim stands until the user
    /// looks. This is the one claim the work-in-flight rule does not touch.
    #[test]
    fn the_bell_outlives_the_work_that_followed_it() {
        let mut ringing = quiet(9);
        ringing.bell_latched = true;
        ringing.working = true;
        ringing.progress = Some(ProgressState::Indeterminate);
        assert_eq!(facts(ringing, 9, false).claim(), StatusClaim::Bell);
        // Even with nothing unread — the bell is not an unread claim.
        assert_eq!(facts(ringing, 99, false).claim(), StatusClaim::Bell);
    }

    /// PIN (T2 J97): unread is "published since you last looked", and the tab
    /// you are looking at is never unread.
    ///
    /// Red gate: without the active-tab clause the tab under the user's eyes
    /// wears a dot asking them to look at it, because its session publishes a
    /// frame on every keystroke.
    #[test]
    fn unread_is_publication_since_the_last_look_and_never_on_the_active_tab() {
        // Behind an inactive tab, new frames are unread.
        assert!(facts(quiet(7), 3, false).has_unseen_output());
        // Caught up: nothing new.
        assert!(!facts(quiet(7), 7, false).has_unseen_output());
        // The active tab is being watched, so publishing *is* seeing.
        assert!(!facts(quiet(7), 3, true).has_unseen_output());
        assert_eq!(facts(quiet(7), 0, true).claim(), StatusClaim::Silent);
        // A failure on the active tab makes no dot either — the same clause
        // covers it, because a failure is a kind of unread.
        let mut failed = quiet(7);
        failed.failure_exit_code = Some(2);
        assert_eq!(facts(failed, 0, true).claim(), StatusClaim::Silent);
        assert_eq!(facts(failed, 0, false).claim(), StatusClaim::Failed);
    }

    /// PIN (T2 D31): the breath is the mock-up's keyframes on the mock-up's
    /// curve — full at the ends, `.28` at the middle, and eased between.
    ///
    /// The shape matters as much as the endpoints: a linear ramp between the
    /// same two values is a flicker, and `ease-in-out` is what makes it read as
    /// breathing. So the curve is checked for its defining property — that it
    /// travels *slowly at the turns and quickly in between* — rather than only
    /// at the keyframes, where a linear ramp would agree exactly.
    #[test]
    fn the_working_breath_runs_the_mock_ups_keyframes_on_its_own_curve() {
        let period = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS);
        let at = |fraction: f32| breathe_opacity(period.mul_f32(fraction), Motion::Full);

        assert!((at(0.0) - 1.0).abs() < 1e-3, "the breath starts full");
        assert!(
            (at(0.5) - WINDOW_TAB_BREATHE_MIN_OPACITY).abs() < 1e-3,
            "the trough is the keyframe's own .28"
        );
        assert!((at(1.0) - 1.0).abs() < 1e-3, "and it returns to full");
        // Cyclic: the second breath is the first one.
        assert!((at(0.25) - at(1.25)).abs() < 1e-3);

        // Never outside the keyframes it interpolates.
        for step in 0..=100 {
            let value = at(step as f32 / 100.0);
            assert!(
                (WINDOW_TAB_BREATHE_MIN_OPACITY..=1.0).contains(&value),
                "the breath left its keyframes at {step}%: {value}"
            );
        }

        // `ease-in-out` is flat at both ends of each half and steepest in the
        // middle of it. Over the first half-breath, the middle fifth must cover
        // more ground than the opening fifth — which is exactly what a linear
        // ramp (equal everywhere) fails.
        let opening = at(0.0) - at(0.1);
        let middle = at(0.2) - at(0.3);
        assert!(
            middle > opening * 2.0,
            "the breath must ease: opening {opening}, middle {middle}"
        );
    }

    /// PIN (T2 D41): with animations off the breath holds one value instead of
    /// stopping at whatever opacity it happened to be passing through.
    ///
    /// The mock-up spells the replacement out (line 1927): `.ticon.working {
    /// opacity: .6 }`. "Working" still has to be legible when nothing may move,
    /// so the answer is a held value, not a still frame and not full opacity.
    #[test]
    fn reduced_motion_holds_the_breath_at_one_value() {
        for fraction in [0.0_f32, 0.25, 0.5, 0.75, 1.0, 3.7] {
            let held = breathe_opacity(
                Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(fraction),
                Motion::Reduced,
            );
            assert!((held - WINDOW_TAB_BREATHE_REDUCED_OPACITY).abs() < 1e-6);
        }
        // And it is genuinely quieter than a mark that is not working at all,
        // which is what makes it still say something.
        const { assert!(WINDOW_TAB_BREATHE_REDUCED_OPACITY < 1.0) };
    }

    /// PIN (T2 D41): the indeterminate arc turns once per its own period, and
    /// stands still — rather than vanishing — when animation is off.
    #[test]
    fn the_indeterminate_arc_turns_once_a_period_and_holds_still_when_asked() {
        let period = Duration::from_millis(WINDOW_TAB_RING_SPIN_PERIOD_MS);
        assert_eq!(
            indeterminate_start_milliturns(Duration::ZERO, Motion::Full),
            0
        );
        assert_eq!(
            indeterminate_start_milliturns(period.mul_f32(0.25), Motion::Full),
            250
        );
        assert_eq!(
            indeterminate_start_milliturns(period.mul_f32(0.5), Motion::Full),
            500
        );
        // A whole turn returns to the start rather than running off the end.
        assert_eq!(indeterminate_start_milliturns(period, Motion::Full), 0);
        assert_eq!(
            indeterminate_start_milliturns(period.mul_f32(7.25), Motion::Full),
            250
        );
        // Stopped, it holds at noon — and it is still an arc. A ring with no
        // arc at all would be reporting 0%, which is a different claim.
        for fraction in [0.0_f32, 0.3, 0.75, 9.1] {
            assert_eq!(
                indeterminate_start_milliturns(period.mul_f32(fraction), Motion::Reduced),
                0
            );
        }
    }

    /// PIN (T2): every `OSC 9;4` state maps to the arc the mock-up gives it.
    ///
    /// The two states that may arrive *without* a percentage are the reason
    /// this takes a `last_sweep`: states 2 and 4 change a run that is already
    /// under way, so the reading already on the wire still stands, and keeping
    /// it is the protocol's own answer rather than an invented number.
    #[test]
    fn each_progress_state_paints_its_own_arc() {
        let palette = LIGHT_CHROME;
        let arc = |state, last| ring_arc(state, last, Duration::ZERO, Motion::Full, &palette);

        let normal = arc(ProgressState::Normal(40), None);
        assert_eq!(normal.color, palette.accent);
        assert_eq!(normal.sweep_milliturns, 400);
        assert!(
            !normal.animating,
            "a determinate arc does not move by itself"
        );

        // Percent is a fraction of the whole turn, at both ends of its range.
        assert_eq!(arc(ProgressState::Normal(0), None).sweep_milliturns, 0);
        assert_eq!(arc(ProgressState::Normal(100), None).sweep_milliturns, 1000);
        // And a report beyond 100 is clamped rather than wrapped — an arc that
        // wrapped would report 130% as 30%.
        assert_eq!(arc(ProgressState::Normal(255), None).sweep_milliturns, 1000);

        // Only the arc's colour changes; the ring is not redrawn as something
        // else (mock-up lines 280-281 recolour `.arc` and nothing more).
        let failed = arc(ProgressState::Error(Some(40)), None);
        assert_eq!(failed.color, palette.status_err);
        assert_eq!(failed.sweep_milliturns, 400);
        let paused = arc(ProgressState::Paused(Some(40)), None);
        assert_eq!(paused.color, palette.status_pause);
        assert_eq!(paused.sweep_milliturns, 400);

        // A state change with no percentage keeps the reading already showing.
        assert_eq!(
            arc(ProgressState::Error(None), Some(400)).sweep_milliturns,
            400
        );
        assert_eq!(
            arc(ProgressState::Paused(None), Some(730)).sweep_milliturns,
            730
        );
        // With no reading ever taken, a full ring — so a failure is visible
        // rather than reported as a bare track.
        assert_eq!(arc(ProgressState::Error(None), None).sweep_milliturns, 1000);

        let spinning = arc(ProgressState::Indeterminate, None);
        assert_eq!(spinning.color, palette.accent);
        assert_eq!(spinning.sweep_milliturns, 243, "13 of the mock-up's 53.4");
        assert!(
            spinning.animating,
            "an indeterminate arc owes the next frame"
        );
        // Stopped, it is the same arc and no longer owes a frame.
        let still = ring_arc(
            ProgressState::Indeterminate,
            None,
            Duration::ZERO,
            Motion::Reduced,
            &palette,
        );
        assert_eq!(still.sweep_milliturns, spinning.sweep_milliturns);
        assert!(!still.animating);
    }

    /// PIN (T2): the arc eases to a new reading instead of snapping to it, and
    /// stops owing frames the moment it arrives.
    ///
    /// `.pring .arc { transition: stroke-dashoffset .3s ease }` (line 279).
    /// The "stops owing frames" half is what keeps an idle window idle: a tween
    /// that never reports itself finished is a 60fps loop that never ends.
    #[test]
    fn the_arc_eases_to_a_new_reading_and_then_stands_down() {
        let started = Instant::now();
        let tween = SweepTween {
            from: 200,
            to: 700,
            started,
        };
        let duration = Duration::from_millis(WINDOW_TAB_RING_SWEEP_TRANSITION_MS);

        let (at_start, moving) = tween.sample(started);
        assert_eq!(at_start, 200, "it begins where the arc already was");
        assert!(moving);

        let (midway, moving) = tween.sample(started + duration / 2);
        assert!(moving);
        assert!(
            (200..=700).contains(&midway),
            "the tween left its endpoints: {midway}"
        );

        let (arrived, moving) = tween.sample(started + duration);
        assert_eq!(arrived, 700, "it arrives exactly, not nearly");
        assert!(!moving, "an arrived tween owes no further frames");
        let (still_there, moving) = tween.sample(started + duration * 4);
        assert_eq!(still_there, 700);
        assert!(!moving);

        // `ease` leaves quickly and arrives slowly, so by the halfway point it
        // is already past halfway. A linear ramp would sit exactly on 450.
        assert!(
            midway > 450,
            "the arc must use CSS `ease`, which front-loads its travel: {midway}"
        );
    }

    /// PIN (T2): the two CSS timing functions are solved, not approximated.
    ///
    /// Both are checked against their defining points — the endpoints every
    /// curve shares, and the midpoint value that tells them apart. `ease` and
    /// `ease-in-out` are symmetric only in the second case, and a solver that
    /// silently returned one for the other would pass every endpoint test.
    #[test]
    fn the_css_timing_curves_are_the_real_beziers() {
        for curve in [EASE, EASE_IN_OUT] {
            assert_eq!(cubic_bezier(0.0, curve), 0.0);
            assert_eq!(cubic_bezier(1.0, curve), 1.0);
            // Out of range in either direction is clamped, not extrapolated.
            assert_eq!(cubic_bezier(-1.0, curve), 0.0);
            assert_eq!(cubic_bezier(2.0, curve), 1.0);
            // Monotonic: time only moves forward, so the curve must too.
            let mut previous = 0.0_f32;
            for step in 0..=200 {
                let value = cubic_bezier(step as f32 / 200.0, curve);
                assert!(value >= previous - 1e-4, "{curve:?} went backwards");
                previous = value;
            }
        }
        // `ease-in-out` is symmetric about its centre and therefore passes
        // through exactly .5 at half time.
        assert!((cubic_bezier(0.5, EASE_IN_OUT) - 0.5).abs() < 1e-3);
        // `ease` is not symmetric: it is already well past half by half time,
        // which is the whole difference between the two and the reason both
        // exist rather than one standing in for the other.
        assert!(cubic_bezier(0.5, EASE) > 0.75);
    }

    /// PIN (T2 D32/D33): each claim wears the mock-up's own colour, and a
    /// silent session draws no dot at all.
    ///
    /// Presence-versus-absence is the point: the mock-up keeps `.unreaddot` in
    /// the DOM always and shows it by class (its comment at line 249 records
    /// why), but what lands on screen is still nothing when there is nothing to
    /// say. A dot drawn in the tab's own colour would be a smudge, not a state.
    #[test]
    fn each_claim_wears_its_own_colour_and_silence_draws_nothing() {
        for palette in [LIGHT_CHROME, DARK_CHROME] {
            assert_eq!(StatusClaim::Silent.dot_color(&palette), None);
            assert_eq!(
                StatusClaim::Unread.dot_color(&palette),
                Some(palette.accent)
            );
            assert_eq!(
                StatusClaim::Bell.dot_color(&palette),
                Some(palette.status_warn)
            );
            assert_eq!(
                StatusClaim::Failed.dot_color(&palette),
                Some(palette.status_err)
            );
            // The three speaking claims are three different colours — a
            // taxonomy that collapses is not a taxonomy.
            let colors =
                [StatusClaim::Unread, StatusClaim::Bell, StatusClaim::Failed].map(|claim| {
                    claim
                        .dot_color(&palette)
                        .expect("a speaking claim has a colour")
                });
            for (index, color) in colors.iter().enumerate() {
                for other in &colors[index + 1..] {
                    assert_ne!(color, other, "two claims cannot share one colour");
                }
            }
        }
    }

    use winit::keyboard::{Key, NamedKey};

    #[test]
    fn theme_mode_resolution_covers_every_os_theme_input() {
        use bt_persist::ThemeModeV1::{Dark, Light, System};
        use winit::window::Theme::{Dark as OsDark, Light as OsLight};

        for (mode, os_theme, expected) in [
            (System, Some(OsDark), Theme::Dark),
            (System, Some(OsLight), Theme::Light),
            (System, None, Theme::Dark),
            (Light, Some(OsDark), Theme::Light),
            (Light, Some(OsLight), Theme::Light),
            (Light, None, Theme::Light),
            (Dark, Some(OsDark), Theme::Dark),
            (Dark, Some(OsLight), Theme::Dark),
            (Dark, None, Theme::Dark),
        ] {
            assert_eq!(resolve_theme_mode(mode, os_theme), expected);
        }
    }

    #[test]
    fn theme_changed_is_ignored_by_explicit_modes_and_resolved_by_system() {
        use bt_persist::ThemeModeV1::{Dark, Light, System};
        use winit::window::Theme::{Dark as OsDark, Light as OsLight};

        assert_eq!(resolved_theme_change(System, OsDark), Some(Theme::Dark));
        assert_eq!(resolved_theme_change(System, OsLight), Some(Theme::Light));
        assert_eq!(resolved_theme_change(Light, OsDark), None);
        assert_eq!(resolved_theme_change(Light, OsLight), None);
        assert_eq!(resolved_theme_change(Dark, OsDark), None);
        assert_eq!(resolved_theme_change(Dark, OsLight), None);
    }

    #[test]
    fn repeated_tab_switches_do_not_feed_window_chrome_back_into_inner_size() {
        let first = Some((260, 160));
        let second = Some((520, 240));
        let aggregate = aggregate_window_minimum([first, second]);
        assert_eq!(aggregate, Some((520, 240)));

        let mut applied = None;
        assert!(window_minimum_changed(&mut applied, aggregate));
        let mut mock_inner_size = PhysicalSize::new(960, 600);
        let stable_inner_size = mock_inner_size;

        for switch in 0..32 {
            let sizes = if switch % 2 == 0 {
                [first, second]
            } else {
                [second, first]
            };
            if window_minimum_changed(&mut applied, aggregate_window_minimum(sizes)) {
                // Model the winit 0.30 Windows behavior that exposed the regression: every setter
                // call re-requests the current client size through non-client adjustment.
                mock_inner_size.height += 40;
            }
            assert_eq!(
                mock_inner_size, stable_inner_size,
                "tab switch {switch} changed the window inner size"
            );
        }
    }

    /// Every DPI Windows can report, from 100% to 300%, including the quarter
    /// steps the display settings offer and the awkward ones a fractional
    /// scaling setting produces.
    const WINDOWS_SCALES: [f64; 8] = [1.0, 1.25, 1.4, 1.5, 1.75, 2.0, 2.5, 3.0];

    fn placement(bounds: WindowBoundsV1, maximized: bool) -> RestoredPlacement {
        RestoredPlacement {
            size: LogicalSize::new(f64::from(bounds.width), f64::from(bounds.height)),
            position: Some(LogicalPosition::new(
                f64::from(bounds.x),
                f64::from(bounds.y),
            )),
            maximized,
        }
    }

    /// The bug this pins: the window grew by one native frame margin — 26x71
    /// physical at 192 DPI — on every single start, because it was saved as an
    /// outer rect and restored as a client size, and winit adds
    /// `AdjustWindowRectExForDpi` to the second. Restart is a fixed point or the
    /// window walks off the screen in a fortnight.
    #[test]
    fn a_window_nobody_touched_is_restored_and_re_saved_byte_for_byte() {
        // Somewhere the OS would have put a window that had no saved position.
        let elsewhere = bt_platform::WindowRect {
            left: 100,
            top: 100,
            right: 900,
            bottom: 700,
        };
        for scale in WINDOWS_SCALES {
            for bounds in [
                WindowBoundsV1 {
                    x: 0,
                    y: 0,
                    width: 960,
                    height: 600,
                },
                // The odd extents and the negative origin of a window parked on a
                // secondary monitor left of the primary one.
                WindowBoundsV1 {
                    x: -1128,
                    y: 66,
                    width: 987,
                    height: 583,
                },
                WindowBoundsV1 {
                    x: 1,
                    y: -3,
                    width: 1,
                    height: 1,
                },
            ] {
                let rect = startup_window_rect(Some(placement(bounds, false)), elsewhere, scale);
                assert_eq!(
                    persisted_window_bounds(rect, scale),
                    bounds,
                    "scale {scale} lost {bounds:?} across one restart"
                );
                // And it stays a fixed point under repetition, which is the shape
                // the bug actually had: a margin added once is invisible, added
                // fifty times it walks the window off the screen.
                let mut generation = bounds;
                for restart in 0..50 {
                    let rect =
                        startup_window_rect(Some(placement(generation, false)), elsewhere, scale);
                    generation = persisted_window_bounds(rect, scale);
                    assert_eq!(
                        generation, bounds,
                        "scale {scale} drifted from {bounds:?} by restart {restart}"
                    );
                }
            }
        }
    }

    /// The restored rectangle is the saved one scaled, and nothing else. Stated
    /// against the exact margin that used to be added, at the DPI it was measured
    /// at: `AdjustWindowRectExForDpi(WS_OVERLAPPEDWINDOW, 192)` is 26x71.
    #[test]
    fn restoring_adds_no_native_frame_margin() {
        let saved = WindowBoundsV1 {
            x: 74,
            y: 74,
            width: 960,
            height: 600,
        };
        let rect = startup_window_rect(
            Some(placement(saved, false)),
            bt_platform::WindowRect {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            },
            2.0,
        );
        assert_eq!(rect.right - rect.left, 1920);
        assert_eq!(rect.bottom - rect.top, 1200);
        assert_eq!((rect.left, rect.top), (148, 148));
    }

    /// §3.1's fallback, in its own words: a rectangle no monitor can see forfeits
    /// its position, and only its position. A size is never off-screen.
    #[test]
    fn a_window_whose_monitor_is_gone_keeps_its_size_where_the_os_opened_it() {
        let opened_at = bt_platform::WindowRect {
            left: 40,
            top: 60,
            right: 640,
            bottom: 460,
        };
        let orphaned = RestoredPlacement {
            size: LogicalSize::new(800.0, 500.0),
            position: None,
            maximized: false,
        };
        let rect = startup_window_rect(Some(orphaned), opened_at, 2.0);
        assert_eq!((rect.left, rect.top), (40, 60));
        assert_eq!(
            (rect.right - rect.left, rect.bottom - rect.top),
            (1600, 1000)
        );
    }

    /// A first run has no rectangle to honour, so the product's opening size is
    /// the one that is stated exactly — as an outer rect, which under the
    /// self-drawn frame is what the user sees.
    #[test]
    fn a_first_run_opens_at_the_products_own_size() {
        let opened_at = bt_platform::WindowRect {
            left: 11,
            top: 22,
            right: 33,
            bottom: 44,
        };
        let rect = startup_window_rect(None, opened_at, 2.0);
        assert_eq!((rect.left, rect.top), (11, 22));
        assert_eq!(
            (rect.right - rect.left, rect.bottom - rect.top),
            ((INITIAL_WIDTH * 2.0) as i32, (INITIAL_HEIGHT * 2.0) as i32)
        );
    }

    #[test]
    fn cursor_blink_resets_flips_and_stays_visible_while_unfocused() {
        let start = Instant::now();
        let mut blink = CursorBlink::new(start);
        assert!(blink.visible());
        assert_eq!(blink.deadline(), Some(start + CURSOR_BLINK_PHASE));

        assert!(blink.advance(start + CURSOR_BLINK_PHASE));
        assert!(!blink.visible(), "the first phase boundary hides the caret");
        let input_at = start + CURSOR_BLINK_PHASE + Duration::from_millis(10);
        assert!(blink.reset(input_at), "input reveals a hidden caret");
        assert!(blink.visible());
        assert_eq!(blink.deadline(), Some(input_at + CURSOR_BLINK_PHASE));

        let unfocused_at = input_at + Duration::from_millis(20);
        blink.set_focused(false, unfocused_at);
        assert!(blink.visible(), "the unfocused outline is always visible");
        assert_eq!(
            blink.deadline(),
            None,
            "unfocused cursors do not wake the loop"
        );
        assert!(!blink.advance(unfocused_at + Duration::from_secs(60)));
        assert!(blink.visible());

        let refocused_at = unfocused_at + Duration::from_secs(61);
        blink.set_focused(true, refocused_at);
        assert!(blink.visible());
        assert_eq!(blink.deadline(), Some(refocused_at + CURSOR_BLINK_PHASE));
    }

    #[test]
    fn cursor_blink_deadline_is_registered_with_the_event_loop_wake_set() {
        let start = Instant::now();
        let blink = CursorBlink::new(start);
        let later = start + Duration::from_secs(10);
        assert_eq!(
            earliest_deadline([Some(later), blink.deadline(), None]),
            blink.deadline()
        );
    }

    #[test]
    fn tab_state_machine_creates_switches_and_closes_to_the_adjacent_tab() {
        let mut tabs = vec!["first"];
        tabs.push("second");
        let mut active = tabs.len() - 1;
        assert_eq!(
            (tabs.as_slice(), active),
            (["first", "second"].as_slice(), 1)
        );

        active = 0;
        assert_eq!(active, 0, "clicking a tab changes only the active index");
        assert_eq!(
            tab_close_action(tabs.len(), active, 0),
            TabCloseAction::Keep { active_tab: 0 },
            "closing the active left tab activates its right neighbour"
        );
        tabs.remove(0);
        assert_eq!(tabs, ["second"]);
        assert_eq!(
            tab_close_action(tabs.len(), 0, 0),
            TabCloseAction::CloseWindow,
            "the last tab delegates to the existing WM_CLOSE path"
        );
    }

    #[test]
    fn closing_a_background_tab_preserves_the_same_active_identity() {
        assert_eq!(
            tab_close_action(4, 2, 0),
            TabCloseAction::Keep { active_tab: 1 }
        );
        assert_eq!(
            tab_close_action(4, 1, 3),
            TabCloseAction::Keep { active_tab: 1 }
        );
    }

    #[test]
    fn input_routes_only_to_the_active_tab_and_background_output_survives_switching() {
        let mut writes = [Vec::new(), Vec::new()];
        let active = 1;
        active_item_mut(&mut writes, active).extend_from_slice(b"whoami\r");
        assert!(writes[0].is_empty());
        assert_eq!(writes[1], b"whoami\r");

        let mut sessions = [
            DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(2).unwrap()),
            DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(2).unwrap()),
        ];
        sessions[0].feed(b"kept in background").unwrap();
        let mut projection = sessions[0].new_projection(sessions[0].layout_key());
        sessions[0].refresh_projection(&mut projection);
        let frame = sessions[0].viewport_frame(&mut projection).unwrap();
        let visible = frame
            .cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        assert!(visible.contains("kept in background"));
    }

    fn hyperlink_hit(uri: &str) -> HyperlinkHit {
        HyperlinkHit {
            id: None,
            uri: uri.to_owned(),
            start: bt_doc::ContentAnchor::Live {
                screen: bt_doc::ScreenId::Primary,
                point: bt_doc::GridPoint { row: 1, column: 2 },
                bias: Bias::Before,
                generation: bt_doc::GridGeneration(1),
            },
            end: bt_doc::ContentAnchor::Live {
                screen: bt_doc::ScreenId::Primary,
                point: bt_doc::GridPoint { row: 1, column: 5 },
                bias: Bias::After,
                generation: bt_doc::GridGeneration(1),
            },
        }
    }

    fn local_selection_route(mode: SelectionDragMode) -> MouseRoute {
        let hit = hyperlink_hit("https://example.test");
        MouseRoute::Local(SelectionDrag {
            mode,
            origin_row: 1,
            origin_column: 2,
            origin: ViewSelection {
                start: hit.start,
                end: hit.end,
            },
            hyperlink: None,
            open_hyperlink_on_release: false,
            local_image_activation: LocalImageActivation::None,
        })
    }

    /// The gear no longer *is* the theme switch — it opens the surface the
    /// switch lives on, and nothing about a caption button decides a colour any
    /// more. The theme now comes from a press on a picker item, which
    /// `settings::theme_requested` answers and `settings.rs` pins.
    ///
    /// Red gate: the previous version of this test asserted the gear returned
    /// the opposite theme. That function is gone, and this one fails the moment
    /// something starts deciding a theme from a `ChromeTarget` again.
    #[test]
    fn the_gear_opens_the_settings_surface_rather_than_deciding_a_theme() {
        let mut panel = settings::SettingsPanel::default();
        panel.toggle();
        assert!(panel.is_open(), "the gear's verb is 'open the dialog'");
        assert_eq!(
            settings::theme_requested(settings::SettingsTarget::Close),
            None,
            "nothing but a picker item asks for a theme"
        );
        assert_eq!(
            settings::theme_requested(settings::SettingsTarget::ThemeOption(ThemeModeV1::Light)),
            Some(ThemeModeV1::Light)
        );
    }

    #[test]
    fn selection_release_copy_policy_covers_drag_word_and_line_but_not_click_or_forwarding() {
        for mode in [
            SelectionDragMode::Linear,
            SelectionDragMode::Word,
            SelectionDragMode::Line,
        ] {
            let route = local_selection_route(mode);
            assert!(should_copy_on_select_release(Some(&route), false));
        }

        let click = local_selection_route(SelectionDragMode::Linear);
        assert!(!should_copy_on_select_release(Some(&click), true));
        let forwarded = MouseRoute::Forward(input::MouseProtocolButton::Left);
        assert!(!should_copy_on_select_release(Some(&forwarded), false));
        assert!(!should_copy_on_select_release(None, false));
    }

    #[test]
    fn hyperlink_activation_requires_ctrl_and_click_without_drag() {
        assert_eq!(
            hyperlink_activation(true, true, "https://example.test/path"),
            HyperlinkActivation::Open
        );
        assert_eq!(
            hyperlink_activation(true, true, "HTTP://localhost:3000"),
            HyperlinkActivation::Open
        );
        assert_eq!(
            hyperlink_activation(false, true, "https://example.test"),
            HyperlinkActivation::None
        );
        assert_eq!(
            hyperlink_activation(true, false, "https://example.test"),
            HyperlinkActivation::None
        );
    }

    #[test]
    fn local_image_click_routes_preview_external_and_no_effect() {
        let verified = std::path::Path::new(r"C:\tmp\decoded.png");
        assert_eq!(
            local_image_activation(false, true, Some(verified)),
            LocalImageActivation::Preview(verified.to_path_buf()),
            "plain click carries the exact hit path into preview"
        );
        assert_eq!(
            local_image_activation(true, true, Some(verified)),
            LocalImageActivation::External(verified.to_path_buf()),
            "Ctrl+click retains the system-viewer verb"
        );
        assert_eq!(
            local_image_activation(false, true, None),
            LocalImageActivation::None,
            "an unmarked cell has no click side effect"
        );
        assert_eq!(
            local_image_activation(false, false, Some(verified)),
            LocalImageActivation::None,
            "dragging remains selection"
        );
    }

    #[test]
    fn hyperlink_activation_blocks_every_non_http_scheme_and_unsafe_target() {
        for uri in [
            "file:///C:/secret.txt",
            "mailto:person@example.test",
            "custom://payload",
            "https://example.test/\nspoof",
        ] {
            assert_eq!(
                hyperlink_activation(true, true, uri),
                HyperlinkActivation::Blocked,
                "{uri:?}"
            );
        }
    }

    #[test]
    fn hyperlink_hover_delay_and_departure_are_event_driven() {
        let start = Instant::now();
        let link = hyperlink_hit("file:///actual-target");
        let mut hover = HyperlinkHover::default();

        // The underline is the immediate affordance: a fresh candidate republishes right away and
        // is the underline target long before the tooltip deadline; only the status text waits.
        assert!(hover.observe(Some(link.clone()), start));
        assert_eq!(hover.underline_target(), Some(&link));
        assert!(hover.active.is_none(), "tooltip must not appear instantly");
        assert!(!hover.activate_if_due(start + Duration::from_millis(299)));
        assert!(hover.activate_if_due(start + Duration::from_millis(300)));
        assert_eq!(
            hover.status_text(80).as_deref(),
            Some("file:///actual-target")
        );
        assert!(hover.observe(None, start + Duration::from_millis(301)));
        assert!(hover.active.is_none());
        assert!(hover.underline_target().is_none());
        assert!(hover.show_at.is_none());

        hover.show_blocked(link);
        assert_eq!(
            hover.status_text(80).as_deref(),
            Some("file:///actual-target · blocked")
        );
        assert_eq!(
            hover.status_text(20).as_deref(),
            Some("file:///a… · blocked"),
            "narrow chrome keeps the real target prefix and the blocked verdict visible"
        );
    }

    #[test]
    fn peek_hover_settles_after_the_delay_and_slides_along_one_span_without_restarting() {
        let start = Instant::now();
        let path = PathBuf::from(r"C:\img\a.png");
        let mut hover = PeekHover::default();

        let subject = PeekSubject::from_path(path.clone());
        assert!(!hover.observe(
            Some(subject.clone()),
            PhysicalPosition::new(10.0, 10.0),
            start
        ));
        assert!(
            hover
                .activate_if_due(start + Duration::from_millis(299))
                .is_none()
        );
        // Sliding along the same path span refreshes the anchor but keeps the original clock:
        // the flyout settles where the pointer last was, without ever restarting the delay.
        assert!(!hover.observe(
            Some(subject.clone()),
            PhysicalPosition::new(30.0, 12.0),
            start + Duration::from_millis(200)
        ));
        let settled = hover
            .activate_if_due(start + Duration::from_millis(300))
            .expect("original deadline must fire");
        assert_eq!(settled.subject, subject);
        assert_eq!(settled.pointer.x, 30.0);
        // While active, staying on the span neither hides nor re-arms.
        assert!(!hover.observe(
            Some(subject.clone()),
            PhysicalPosition::new(31.0, 12.0),
            start + Duration::from_millis(400)
        ));
        assert!(hover.show_at.is_none());
        // Leaving the span hides the flyout and drops all state.
        assert!(hover.observe(
            None,
            PhysicalPosition::new(31.0, 40.0),
            start + Duration::from_millis(500)
        ));
        assert!(hover.active.is_none());
    }

    #[test]
    fn peek_hover_switching_paths_hides_the_old_flyout_and_restarts_the_clock() {
        let start = Instant::now();
        let first = PeekSubject::from_path(PathBuf::from(r"C:\img\a.png"));
        let second = PeekSubject::from_path(PathBuf::from(r"C:\img\b.png"));
        let mut hover = PeekHover::default();
        hover.observe(Some(first), PhysicalPosition::new(10.0, 10.0), start);
        assert!(
            hover
                .activate_if_due(start + Duration::from_millis(300))
                .is_some()
        );
        let hidden = hover.observe(
            Some(second.clone()),
            PhysicalPosition::new(50.0, 10.0),
            start + Duration::from_millis(400),
        );
        assert!(hidden, "switching spans must hide the visible flyout");
        assert!(
            hover
                .activate_if_due(start + Duration::from_millis(600))
                .is_none(),
            "the second span runs a fresh settle clock"
        );
        let settled = hover
            .activate_if_due(start + Duration::from_millis(700))
            .expect("second span settles on its own deadline");
        assert_eq!(settled.subject, second);
    }

    /// PIN (user repro 2026-08-02, re-seated by the frame-derived ruling 2026-08-04): one hovered
    /// cell resolves to one reference, through the frame's own row stride, and where a printed path
    /// and a link target cover the same cell the pointer answers with the text it is standing on.
    ///
    /// What the four verbs share is this lookup: the scan produced the list, and hover, click and
    /// peek all ask it the same question about the same `GridHit`. The list's own contents — which
    /// shapes are in it, and that a `file://` to a `.txt` is in none — are bt-term's to pin, beside
    /// the detector that decides it (`underline_coverage_equals_peek_coverage_for_every_shape`).
    #[test]
    fn one_hit_resolves_to_one_reference_through_the_frames_own_stride() {
        let printed = PathBuf::from(r"D:\from-text.png");
        let linked = PathBuf::from(r"D:\layout-preview.png");
        let references = FrameImageReferences {
            columns: 10,
            references: vec![
                // The scan's order: printed text first, link targets after it.
                bt_term::FrameImageReference {
                    path: printed.clone(),
                    cells: vec![12, 13, 14],
                    verified: true,
                },
                bt_term::FrameImageReference {
                    path: linked.clone(),
                    cells: vec![10, 11, 12, 13, 14, 15],
                    verified: true,
                },
            ],
        };
        let at = |row, column| {
            references
                .at(bt_render::GridHit { row, column })
                .map(|reference| reference.path.clone())
        };
        assert_eq!(
            at(1, 2),
            Some(printed),
            "text under the pointer is what the pointer was put on",
        );
        assert_eq!(
            at(1, 0),
            Some(linked),
            "and the link answers where its label spells no file",
        );
        assert_eq!(at(1, 6), None, "one column past the link is ordinary text");
        assert_eq!(at(0, 2), None, "the row is part of the address");
        assert_eq!(
            FrameImageReferences::default().at(bt_render::GridHit { row: 0, column: 0 }),
            None,
            "a frame with no references answers nothing anywhere",
        );
    }

    /// PIN (band retirement ruling, 2026-08-03, docs §6.1): the peek's third source is an OSC 1337
    /// payload, which names no file, and the pipeline tells the two apart by exactly one property —
    /// whether a cache miss has anything to read.
    ///
    /// A named file's identity is its normalized path, so the same file spelled two ways is one
    /// hover and one cache entry. A payload's identity is the decoder's content key, and it carries
    /// no path at all: the bytes came through the stream and were remembered when the decode landed,
    /// so a miss is a hover that arrived early, never a disk read to schedule. That `path: None` is
    /// the whole of the difference is what keeps `show_or_request_peek` one function.
    #[test]
    fn a_stream_payload_is_a_peek_subject_with_nothing_to_read() {
        let by_path = PeekSubject::from_path(PathBuf::from(r"C:\img\a.png"));
        assert_eq!(
            by_path.key,
            normalized_local_image_path_key(std::path::Path::new(r"C:\img\a.png")),
            "a named file is identified the way the decoder identifies it",
        );
        assert!(by_path.path.is_some(), "a named file is readable on a miss");
        assert_eq!(
            PeekSubject::from_path(PathBuf::from(r"C:\IMG\A.PNG")),
            by_path,
            "one file spelled two ways is one hover and one cache entry",
        );

        let payload = PeekSubject::from_content_key("image:sha-abc".to_owned());
        assert_eq!(payload.key, "image:sha-abc");
        assert!(
            payload.path.is_none(),
            "a stream payload has no file behind it, so a cache miss reads nothing",
        );
        assert_ne!(payload, by_path);
    }

    /// PIN (verification ruling 2026-08-04, the warm peek): the decode a verified reference already
    /// paid for is filed under the very key the hover looks up, so the flyout opens from cache and
    /// no second read of the same file is ever scheduled.
    ///
    /// `show_or_request_peek` sends a `PeekImage` task on exactly one condition — a `None` entry
    /// under `PeekSubject::key`. So "the peek is warm" and "the two keys are the same string" are
    /// the same statement, and it is the one asserted here. The stream-payload arm is asserted
    /// beside it because both shapes go through this one function and must not converge: a payload
    /// has no path to key by.
    ///
    /// RED CHECK: keying a named file's verification decode by `decoded.key` (its content identity)
    /// instead of its path leaves the hover's lookup missing, and the first assertion goes red —
    /// which is precisely the "decoded twice, cached twice" defect the shared key rules out.
    #[test]
    fn a_verified_references_decode_is_filed_under_the_key_the_hover_asks_by() {
        let path = PathBuf::from(r"C:\img\Sunset.PNG");
        let decoded = bt_term::DecodedInlineImage {
            occurrence_id: 7,
            key: "image:0123456789abcdef0123456789abcdef".to_owned(),
            rgba: Arc::from(vec![0u8; 4]),
            width_px: 1,
            height_px: 1,
            animated: false,
        };

        assert_eq!(
            peek_cache_key_for_decode(
                &bt_term::InlineImageSource::LocalPath(path.clone()),
                &decoded
            ),
            PeekSubject::from_path(path.clone()).key,
            "the verification decode lands exactly where the hover will look for it",
        );
        assert_eq!(
            peek_cache_key_for_decode(
                &bt_term::InlineImageSource::LocalPath(PathBuf::from(r"c:/img/sunset.png")),
                &decoded
            ),
            PeekSubject::from_path(path).key,
            "and one file spelled two ways is still one warm entry",
        );
        assert_eq!(
            peek_cache_key_for_decode(
                &bt_term::InlineImageSource::Osc1337(b"AAAA".to_vec()),
                &decoded
            ),
            PeekSubject::from_content_key(decoded.key.clone()).key,
            "a stream payload has no path, so it stays keyed by content",
        );
    }

    /// Pin (a) of the peek raster defect: every peek pixel that reaches the renderer is one the
    /// flyout draws. The chain the app runs — the renderer's box, the worker's resample, the
    /// thumbnail slot, the overlay — is walked end to end here, so a future edit that hands the
    /// renderer a native decode again fails on the resident byte count and on the texture key.
    #[test]
    fn the_peek_overlay_carries_display_sized_pixels_under_a_display_sized_key() {
        // A decode far larger than any flyout: 1024x768 in a 640x480 pane.
        let (native_width_px, native_height_px) = (1024_u32, 768_u32);
        let native_rgba: Arc<[u8]> =
            Arc::from(vec![
                0x40_u8;
                native_width_px as usize * native_height_px as usize * 4
            ]);
        let content_key = "image:0123456789abcdef0123456789abcdef".to_owned();

        let (display_width_px, display_height_px) = bt_render::peek_thumbnail_extent(
            640.0,
            480.0,
            8.0,
            1.0,
            native_width_px,
            native_height_px,
        )
        .expect("the pane can host the flyout");
        assert!(
            display_width_px < native_width_px && display_height_px < native_height_px,
            "the 40% cap is what makes the flyout smaller than its decode",
        );

        let target: PeekThumbnailTarget =
            (content_key.clone(), display_width_px, display_height_px);
        let task = peek_scale_task(
            &target,
            Arc::clone(&native_rgba),
            native_width_px,
            native_height_px,
        );
        let thumbnail = PeekThumbnail::from_scaled(bt_term::scale_inline_image(&task));
        let overlay = thumbnail.overlay(PhysicalPosition::new(120.0, 90.0));
        assert_eq!(
            (overlay.width_px, overlay.height_px),
            (display_width_px, display_height_px),
        );
        assert_eq!(
            overlay.rgba.len(),
            display_width_px as usize * display_height_px as usize * 4,
            "the resident bytes the renderer uploads are the display box, not the decode",
        );
        assert!(
            overlay.rgba.len() * 16 < native_rgba.len(),
            "the defect uploaded {} bytes where {} suffice",
            native_rgba.len(),
            overlay.rgba.len(),
        );
        assert_eq!(
            overlay.key,
            bt_term::display_texture_key(&content_key, display_width_px, display_height_px),
            "the display size is part of the texture identity, so the shared LRU can never \
             serve a raster sized for another box",
        );
        assert!(
            thumbnail.matches(&target),
            "the slot answers the question the hover asked, so a raster for another box is \
             never presented as this one",
        );
    }

    #[test]
    fn osc_8_display_text_hits_the_real_target_uri() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(24).unwrap(), NonZeroU32::new(2).unwrap());
        session
            .feed(b"\x1b]8;;https://actual.example/login\x1b\\trusted label\x1b]8;;\x1b\\")
            .unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let mut frame = session.viewport_frame(&mut projection).unwrap();

        let hit = frame.hyperlink_at(0, 4).unwrap();
        assert_eq!(hit.uri, "https://actual.example/login");
        assert!(frame.underline_hyperlink(&hit));
        assert!(frame.cells[..13].iter().all(|cell| {
            cell.style
                .flags
                .contains(bt_transcript::CellFlags::UNDERLINE)
        }));
        assert!(
            !frame.cells[13]
                .style
                .flags
                .contains(bt_transcript::CellFlags::UNDERLINE)
        );
    }

    struct PtyPresentationHarness {
        session: DualPlaneSession,
        projection: ViewportProjection,
        pending: LatestFrameSlot,
        last_presented: Option<ViewportFrame>,
        publications: usize,
    }

    impl PtyPresentationHarness {
        fn new(columns: u32, rows: u32) -> Self {
            let session = DualPlaneSession::new(
                NonZeroU32::new(columns).unwrap(),
                NonZeroU32::new(rows).unwrap(),
            );
            let projection = session.new_projection(session.layout_key());
            Self {
                session,
                projection,
                pending: LatestFrameSlot::default(),
                last_presented: None,
                publications: 0,
            }
        }

        fn feed_drain(&mut self, bytes: &[u8]) -> bool {
            self.session.feed(bytes).unwrap();
            self.publish_pty_frame()
        }

        fn finish_synchronized_update(&mut self) -> (bool, bool) {
            let finished = self
                .session
                .finish_synchronized_update(Instant::now())
                .unwrap();
            let published = finished && self.publish_pty_frame();
            (finished, published)
        }

        fn publish_pty_frame(&mut self) -> bool {
            self.session.refresh_projection(&mut self.projection);
            let frame = self.session.viewport_frame(&mut self.projection).unwrap();
            // Mirror publish_frame_inner's combined review/exact-source presentation hold.
            if self.projection.presentation_hold() && self.last_presented.is_some() {
                return false;
            }
            if pty_frame_is_unchanged(
                self.pending.pending_frame(),
                self.last_presented.as_ref(),
                &frame,
            ) {
                return false;
            }
            self.pending
                .publish(
                    frame,
                    FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::PtyOutput,
                    },
                )
                .unwrap();
            self.publications += 1;
            true
        }

        fn present_pending(&mut self) -> bool {
            let Some((frame, _)) = self.pending.take() else {
                return false;
            };
            self.last_presented = Some(frame);
            true
        }
    }

    fn frame_row_text(frame: &ViewportFrame, row: usize) -> String {
        let columns = frame.columns.get() as usize;
        frame.cells[row * columns..(row + 1) * columns]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect()
    }

    #[test]
    fn a_resize_reflow_holds_until_projectable_staging_reanchors_the_reprint() {
        let start = Instant::now();
        let mut harness = PtyPresentationHarness::new(40, 10);
        let mut lines = Vec::new();
        for index in 0..60 {
            lines.extend_from_slice(format!("line-{index:03}\r\n").as_bytes());
        }
        harness.session.feed(&lines).unwrap();
        assert!(harness.publish_pty_frame());
        harness.present_pending();

        // Enter review.
        harness.projection.scroll_by_rows(20);
        assert!(harness.publish_pty_frame());
        harness.present_pending();
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20
        );
        let publications_before = harness.publications;

        // A resize opens the transaction and Codex clears scrollback: the review anchor vanishes
        // and history is transiently empty. The interim frame is bottom-snapped, but presentation
        // must hold the last frame rather than flash to the bottom.
        harness
            .session
            .resize_at(
                NonZeroU32::new(40).unwrap(),
                NonZeroU32::new(12).unwrap(),
                start,
            )
            .unwrap();
        harness.session.mark_pty_resize_requested_at(
            NonZeroU32::new(40).unwrap(),
            NonZeroU32::new(12).unwrap(),
            start + Duration::from_millis(10),
        );
        harness
            .session
            .feed_at(b"\x1b[2J\x1b[3J\x1b[H", start + Duration::from_millis(20))
            .unwrap();
        assert!(
            !harness.publish_pty_frame(),
            "the hold skips publishing the bottom-snapped interim frame"
        );
        assert!(
            !harness.present_pending(),
            "nothing was published to present"
        );
        assert_eq!(
            harness.publications, publications_before,
            "no new publication during the hold"
        );
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20,
            "the screen still shows the reviewing frame, not the bottom"
        );

        // The reprint enters projectable resize staging: publication resumes immediately at the
        // restored review position — a direct hand-off with no bottom frame ever presented.
        harness
            .session
            .feed_at(&lines, start + Duration::from_millis(30))
            .unwrap();
        assert!(
            harness.publish_pty_frame(),
            "resize staging releases the hold as soon as the reprint is reachable"
        );
        harness.present_pending();
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20,
            "presentation resumes exactly at the staged review displacement"
        );

        // Quiescence commits the same staging ids through normal history relocation and must not
        // move the already-restored reading position.
        assert!(
            harness
                .session
                .finish_resize_if_quiescent(start + Duration::from_millis(280))
                .unwrap()
        );
        if harness.publish_pty_frame() {
            harness.present_pending();
        }
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20,
            "final harvest preserves the already-restored review displacement"
        );
    }

    #[test]
    fn a_late_zoom_reprint_keeps_the_last_formula_frame_until_exact_source_reanchors() {
        let start = Instant::now();
        let mut harness = PtyPresentationHarness::new(40, 24);
        harness
            .session
            .feed_at(b"intro\r\n$$x$$\r\nbarrier", start)
            .unwrap();
        assert_eq!(
            harness
                .session
                .advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
            1
        );
        let mut initial_task = harness.session.take_live_worker_task().unwrap();
        let initial_raster =
            render_live_detection_task(&MathEngine::new(), &mut initial_task, foreground_rgb())
                .expect("initial formula rasterizes");
        assert!(
            harness
                .session
                .complete_live_worker_result(initial_task, Ok(initial_raster))
        );
        assert!(harness.publish_pty_frame());
        assert!(harness.present_pending());
        assert_eq!(
            harness.last_presented.as_ref().unwrap().math_blocks.len(),
            1
        );

        // Match reconcile_authoritative_dpi: metrics, grid resize, then the new-DPI layout key.
        let zoom_at = start + Duration::from_millis(210);
        harness.session.set_cell_height_subpixels(
            NonZeroI64::new(14 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        harness
            .session
            .set_cell_width_subpixels(NonZeroI64::new(7 * bt_viewport::SUBPIXELS_PER_PX).unwrap());
        harness.session.set_ascii_baseline_subpixels(
            NonZeroI64::new(11 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        harness
            .session
            .resize_at(
                NonZeroU32::new(52).unwrap(),
                NonZeroU32::new(32).unwrap(),
                zoom_at,
            )
            .unwrap();
        harness.session.mark_pty_resize_requested_at(
            NonZeroU32::new(52).unwrap(),
            NonZeroU32::new(32).unwrap(),
            zoom_at,
        );
        harness.session.set_layout_key(bt_doc::LayoutKey {
            width_cells: NonZeroU32::new(52).unwrap(),
            dpi_milli: NonZeroU32::new(800).unwrap(),
            font_rev: 1,
            theme_rev: harness.session.layout_key().theme_rev,
        });
        let delayed_relayout = harness.session.take_live_worker_task().unwrap();
        assert!(harness.publish_pty_frame());
        assert!(harness.present_pending());
        assert!(
            harness
                .session
                .finish_resize_if_quiescent(zoom_at + Duration::from_millis(300))
                .unwrap()
        );

        let publications_before_gap = harness.publications;
        harness
            .session
            .feed_at(
                b"\x1b[2J\x1b[H\x1b[3Jintro\r\n$",
                zoom_at + Duration::from_millis(310),
            )
            .unwrap();
        assert!(
            !harness.publish_pty_frame(),
            "publish_frame_inner must skip the diagnosed incomplete reprint frame"
        );
        assert!(!harness.present_pending());
        assert_eq!(harness.publications, publications_before_gap);
        assert_eq!(
            harness.last_presented.as_ref().unwrap().math_blocks.len(),
            1,
            "the swapchain remains on the last complete formula frame"
        );

        harness
            .session
            .feed_at(b"$x$$\r\nbarrier", zoom_at + Duration::from_millis(324))
            .unwrap();
        let reanchor_published = harness.publish_pty_frame();
        assert!(
            !harness.projection.presentation_hold(),
            "exact-source re-anchor releases the hold immediately"
        );
        if reanchor_published {
            assert!(harness.present_pending());
        }
        assert_eq!(
            harness.last_presented.as_ref().unwrap().math_blocks.len(),
            1
        );

        // The re-anchored stale frame may be content-identical to last_presented and therefore need
        // no publication. Either way, no incomplete grid frame entered the presentation slot.
        drop(delayed_relayout);
    }

    #[test]
    fn keyboard_mapping_is_ascii_only_and_preserves_terminal_controls() {
        assert_eq!(
            input::keyboard_bytes(
                &Key::Character("hello".into()),
                ModifiersState::empty(),
                false
            ),
            Some(b"hello".to_vec())
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Named(NamedKey::Enter), ModifiersState::empty(), false),
            Some(vec![b'\r'])
        );
        assert_eq!(
            input::keyboard_bytes(
                &Key::Named(NamedKey::Backspace),
                ModifiersState::empty(),
                false
            ),
            Some(vec![0x7f])
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Named(NamedKey::Space), ModifiersState::empty(), false),
            Some(vec![b' '])
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Character("c".into()), ModifiersState::CONTROL, false),
            Some(vec![0x03])
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Character("x".into()), ModifiersState::CONTROL, false),
            None
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Character("中".into()), ModifiersState::empty(), false),
            None
        );
        assert_eq!(
            input::keyboard_bytes(
                &Key::Named(NamedKey::Process),
                ModifiersState::CONTROL,
                false
            ),
            None
        );
    }

    #[test]
    fn ime_commit_is_the_exact_utf8_pty_payload() {
        assert_eq!(
            ime_commit_bytes("你好世界"),
            vec![
                0xe4, 0xbd, 0xa0, 0xe5, 0xa5, 0xbd, 0xe4, 0xb8, 0x96, 0xe7, 0x95, 0x8c,
            ]
        );
    }

    #[test]
    fn committed_utf8_projects_as_alacritty_wide_lead_and_spacer_cells() {
        let mut session = DualPlaneSession::with_quotas_and_cell_height(
            NonZeroU32::new(8).unwrap(),
            NonZeroU32::new(2).unwrap(),
            DEFAULT_STAGING_QUOTA,
            M0_FROZEN_LINE_QUOTA,
            std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        session.feed(&ime_commit_bytes("A你B")).unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();

        assert_eq!(frame.cells[0].text, "A");
        assert_eq!(frame.cells[1].text, "你");
        assert!(
            frame.cells[1]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        assert!(frame.cells[2].wide_spacer);
        assert_eq!(frame.cells[3].text, "B");
    }

    #[test]
    fn bracketed_paste_follows_vendor_decset_and_normalizes_crlf() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(2).unwrap());

        session.feed(b"\x1b[?2004h").unwrap();
        assert_eq!(
            input::paste_bytes("one\r\ntwo\n", session.bracketed_paste_mode()),
            b"\x1b[200~one\rtwo\r\x1b[201~"
        );

        session.feed(b"\x1b[?2004l").unwrap();
        assert_eq!(
            input::paste_bytes("one\r\ntwo\n", session.bracketed_paste_mode()),
            b"one\rtwo\r"
        );
    }

    #[test]
    fn unavailable_clipboard_copy_keeps_selection_and_allows_a_retry() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"retry me").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let selection = ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
        };
        session.set_view_selection(Some(selection.clone()));
        projection.set_selection(Some(selection));

        let copied = copy_selection(&mut session, &mut projection, |_| {
            Err(anyhow!("injected clipboard owner contention"))
        });
        assert!(
            !copied,
            "clipboard contention must not escape as a fatal error"
        );
        assert_eq!(session.selection_text().as_deref(), Some("retry me"));
        assert!(projection.selection().is_some());

        let mut clipboard = String::new();
        let copied = copy_selection(&mut session, &mut projection, |text| {
            clipboard.push_str(text);
            Ok(())
        });
        assert!(copied);
        assert_eq!(clipboard, "retry me");
        assert!(session.view_selection().is_none());
        assert!(projection.selection().is_none());
    }

    #[test]
    fn ctrl_c_keeps_its_existing_empty_text_write_and_clear_semantics() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"   ").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let selection = ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 2, Bias::After).unwrap().unwrap(),
        };
        session.set_view_selection(Some(selection.clone()));
        projection.set_selection(Some(selection));
        let mut writes = Vec::new();

        assert!(copy_selection(&mut session, &mut projection, |text| {
            writes.push(text.to_owned());
            Ok(())
        }));
        assert_eq!(writes, [""]);
        assert!(session.view_selection().is_none());
        assert!(projection.selection().is_none());
    }

    #[test]
    fn copy_on_select_writes_nonempty_text_and_keeps_the_selection() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"drag me").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let selection = ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 6, Bias::After).unwrap().unwrap(),
        };
        session.set_view_selection(Some(selection));
        let mut clipboard = String::new();

        assert!(write_selection_text(&session, true, |text| {
            clipboard.push_str(text);
            Ok(())
        }));
        assert_eq!(clipboard, "drag me");
        assert_eq!(session.selection_text().as_deref(), Some("drag me"));
    }

    #[test]
    fn copy_on_select_does_not_touch_the_clipboard_for_an_empty_selection() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"click").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let anchor = frame.anchor_at(0, 2, Bias::Before).unwrap().unwrap();
        session.set_view_selection(Some(ViewSelection {
            start: anchor.clone(),
            end: anchor,
        }));
        let mut writes = 0;

        assert!(!write_selection_text(&session, true, |_| {
            writes += 1;
            Ok(())
        }));
        assert_eq!(writes, 0);
    }

    #[test]
    fn unavailable_clipboard_during_copy_on_select_keeps_the_selection() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"retry me").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        session.set_view_selection(Some(ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
        }));

        assert!(!write_selection_text(&session, true, |_| {
            Err(anyhow!("injected clipboard owner contention"))
        }));
        assert_eq!(session.selection_text().as_deref(), Some("retry me"));
    }

    #[test]
    fn unavailable_clipboard_paste_keeps_state_and_allows_a_retry() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"selected").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let selection = ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
        };
        session.set_view_selection(Some(selection.clone()));
        projection.set_selection(Some(selection));
        let mut pty_writes = Vec::new();

        assert!(
            !paste_from_clipboard(
                &mut session,
                &mut projection,
                || Err(anyhow!("injected clipboard owner contention")),
                |chunk| {
                    pty_writes.extend_from_slice(chunk);
                    Ok(())
                },
            )
            .unwrap()
        );
        assert!(pty_writes.is_empty());
        assert!(session.view_selection().is_some());
        assert!(projection.selection().is_some());

        assert!(
            paste_from_clipboard(
                &mut session,
                &mut projection,
                || Ok("paste me".to_owned()),
                |chunk| {
                    pty_writes.extend_from_slice(chunk);
                    Ok(())
                },
            )
            .unwrap()
        );
        assert_eq!(pty_writes, b"paste me");
        assert!(session.view_selection().is_none());
        assert!(projection.selection().is_none());
    }

    #[test]
    fn unavailable_system_wheel_setting_uses_the_windows_default() {
        assert_eq!(
            recoverable_wheel_scroll_amount(Err("injected SPI failure".to_owned())),
            bt_platform::WheelScrollAmount::Lines(3)
        );
    }

    #[test]
    fn wheel_accumulator_preserves_fractional_residue_across_events() {
        // Eight trackpad ticks of 0.375 units (binary-exact) must add up to exactly 3 whole
        // units drained, never 0 (per-event truncation) and never 4 (double counting).
        let mut remainder = 0.0;
        let mut drained = 0;
        for _ in 0..8 {
            remainder += 0.375;
            drained += drain_whole_units(&mut remainder, 1.0);
        }
        assert_eq!(drained, 3);
        assert_eq!(remainder, 0.0);

        // Whole-line wheel notches with a 17px cell drain exactly one line per 17px, residue 8.
        let mut pixels = 25.0;
        assert_eq!(drain_whole_units(&mut pixels, 17.0), 1);
        assert!((pixels - 8.0).abs() < 1e-9);
    }

    #[test]
    fn wheel_accumulator_truncates_symmetrically_and_never_flips_sign_on_reversal() {
        // +0.6 then -0.7: neither direction has accrued a whole unit, so nothing drains and the
        // residue nets out — reversal must not manufacture a step from opposite-sign residue.
        let mut remainder = 0.0;
        remainder += 0.6;
        assert_eq!(drain_whole_units(&mut remainder, 1.0), 0);
        remainder += -0.7;
        assert_eq!(drain_whole_units(&mut remainder, 1.0), 0);
        assert!((remainder + 0.1).abs() < 1e-9);

        // A full downward unit drains as -1 with the same magnitude rules as upward.
        let mut down = -1.4;
        assert_eq!(drain_whole_units(&mut down, 1.0), -1);
        assert!((down + 0.4).abs() < 1e-9);
    }

    #[test]
    fn disconnected_math_dispatch_downgrades_once_and_leaves_the_real_session_usable() {
        let start = Instant::now();
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(40).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed_at(b"$$x$$\x1b[?25l", start).unwrap();
        assert_eq!(
            session.advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
            1
        );
        let (tasks, receiver) = mpsc::channel();
        drop(receiver);
        let (scale_tasks, _scale_receiver) = mpsc::channel();
        let mut running = true;
        let mut notice_pending = false;

        assert!(dispatch_pending_math_tasks(
            TabId(1),
            &mut session,
            &tasks,
            &scale_tasks,
            &mut running,
            &mut notice_pending,
        ));
        assert!(!running);
        assert!(notice_pending);
        session.feed(b"\r\nterminal-still-running").unwrap();
        assert!(
            session
                .terminal()
                .visible_text()
                .iter()
                .any(|row| row.contains("terminal-still-running"))
        );

        assert_eq!(
            take_math_worker_notice(&mut notice_pending),
            Some(MATH_WORKER_STOPPED_NOTICE)
        );
        assert!(!notice_pending);
        assert_eq!(take_math_worker_notice(&mut notice_pending), None);
        assert!(!dispatch_pending_math_tasks(
            TabId(1),
            &mut session,
            &tasks,
            &scale_tasks,
            &mut running,
            &mut notice_pending,
        ));
        assert!(
            !notice_pending,
            "the user-visible downgrade notice is one-shot"
        );
    }

    fn scale_task(content_key: &str, width: u32) -> bt_term::InlineImageScaleTask {
        bt_term::InlineImageScaleTask {
            occurrence_id: 0,
            content_key: content_key.to_owned(),
            rgba: Arc::from([0_u8, 0, 0, 255]),
            width_px: 1,
            height_px: 1,
            display_width_px: width,
            display_height_px: width,
        }
    }

    /// RED: a divider storm used to put every intermediate Lanczos3 request on the one FIFO.
    /// The worker must execute only the newest size for one content/purpose, while preserving a
    /// different purpose as an independent question.
    #[test]
    fn scale_worker_drag_storm_discards_superseded_work_and_completes_the_latest() {
        let (sender, receiver) = mpsc::channel();
        for width in 1..=128 {
            sender
                .send(ScaleWorkerRequest::Preview {
                    tab_id: TabId(1),
                    task: scale_task("same-path", width),
                })
                .unwrap();
        }
        sender
            .send(ScaleWorkerRequest::Peek {
                tab_id: TabId(1),
                task: scale_task("same-path", 17),
            })
            .unwrap();
        drop(sender);

        let mut executed = Vec::new();
        run_scale_worker(receiver, |request| {
            executed.push((request.purpose(), request.task().display_width_px));
        });

        assert_eq!(
            executed,
            vec![(ScalePurpose::Preview, 128), (ScalePurpose::Peek, 17)]
        );
    }

    /// RED: an obsolete completion must not clear the newest pending target, and the newest
    /// completion must retire "Loading image..." instead of leaving the preview stuck forever.
    #[test]
    fn preview_loading_survives_a_stale_scale_answer_then_clears_on_the_latest() {
        let mut preview = PreviewImageState::new(PathBuf::from("storm.png"));
        preview.pending = Some(("same-path".to_owned(), 320, 180));

        assert!(
            !preview.accept_scaled(bt_term::scale_inline_image(&scale_task("same-path", 160,)))
        );
        assert_eq!(preview.message(), Some("Loading storm.png…".to_owned()));

        assert!(preview.accept_scaled(bt_term::scale_inline_image(
            &bt_term::InlineImageScaleTask {
                display_width_px: 320,
                display_height_px: 180,
                ..scale_task("same-path", 320)
            },
        )));
        assert_eq!(preview.message(), None);
    }

    #[test]
    fn preview_resize_storm_reuses_the_shared_quiet_boundary() {
        let start = Instant::now();
        let last = start + Duration::from_millis(90);
        let mut preview = PreviewImageState::new(PathBuf::from("storm.png"));
        preview.defer_resize_scale(start);
        preview.defer_resize_scale(start + Duration::from_millis(40));
        preview.defer_resize_scale(last);

        assert!(
            !preview.finish_resize_scale_if_quiet(
                last + WINDOW_RESIZE_QUIET - Duration::from_millis(1)
            )
        );
        assert!(preview.finish_resize_scale_if_quiet(last + WINDOW_RESIZE_QUIET));
        assert_eq!(preview.resize_scale_deadline, None);
        assert!(!preview.finish_resize_scale_if_quiet(last + WINDOW_RESIZE_QUIET));
    }

    /// A scale worker may spend arbitrarily long inside Lanczos3; validation still reaches the
    /// independent decoration receiver instead of sitting behind that raster in one FIFO.
    #[test]
    fn local_path_validation_and_resampling_are_dispatched_to_independent_lanes() {
        let (tasks, task_receiver) = mpsc::channel();
        let (scale_tasks, scale_receiver) = mpsc::channel();
        assert!(dispatch_decoration_task(
            TabId(1),
            SessionDecorationTask::ScaleInlineImage(scale_task("same-path", 128)),
            &tasks,
            &scale_tasks,
        ));
        assert!(dispatch_decoration_task(
            TabId(1),
            SessionDecorationTask::InlineImage(bt_term::InlineImageTask {
                occurrence_id: 7,
                source: bt_term::InlineImageSource::LocalPath(PathBuf::from("same-path.png")),
            }),
            &tasks,
            &scale_tasks,
        ));

        assert!(matches!(
            scale_receiver.try_recv(),
            Ok(ScaleWorkerRequest::InlineImage { .. })
        ));
        assert!(matches!(
            task_receiver.try_recv(),
            Ok(MathWorkerRequest::InlineImage {
                task: bt_term::InlineImageTask {
                    occurrence_id: 7,
                    ..
                },
                ..
            })
        ));
    }

    /// The caret rectangle leaves the frame in the terminal seat's coordinates
    /// and reaches winit and IMM32 in the window's. This is the one place the
    /// seat correction runs in that direction, so it is pinned in both: a lone
    /// leaf's origin is `(0, 0)` and the number is unchanged, and a seat that
    /// has been moved carries the caret with it.
    ///
    /// Red gate: return `area` unchanged and the second case fails — the
    /// candidate window would open a seat's width away from the caret.
    #[test]
    fn the_ime_caret_leaves_the_seat_in_the_windows_coordinates() {
        let area = ImeCursorArea {
            x: 250,
            y: 100,
            width: 18,
            height: 44,
        };
        assert_eq!(
            window_ime_cursor_area(SeatViewport::whole(1920, 1200), area),
            area,
            "a lone leaf's seat is the window, so nothing moves"
        );
        let moved = window_ime_cursor_area(
            SeatViewport {
                x: 976,
                y: 0,
                width: 944,
                height: 1200,
            },
            area,
        );
        assert_eq!(
            moved,
            ImeCursorArea {
                x: 250 + 976,
                y: 100,
                width: 18,
                height: 44,
            },
            "a seat with an origin carries the caret to the window's axis"
        );
    }

    #[test]
    fn ime_cursor_area_throttle_coalesces_to_sixty_hz_and_flushes_the_last_area() {
        let start = Instant::now();
        let first = ImeCursorArea {
            x: 10,
            y: 20,
            width: 9,
            height: 22,
        };
        let latest = ImeCursorArea { x: 30, ..first };
        let mut throttle = ImeCursorThrottle::default();

        assert_eq!(throttle.offer(first, start), Some(first));
        assert_eq!(
            throttle.offer(latest, start + Duration::from_millis(3)),
            None
        );
        assert_eq!(throttle.flush_due(start + Duration::from_millis(15)), None);
        assert_eq!(
            throttle.flush_due(start + IME_CURSOR_AREA_INTERVAL),
            Some(latest)
        );
        assert_eq!(throttle.deadline(), None);
    }

    #[test]
    fn startup_polls_pty_until_the_first_text_frame_is_presented() {
        assert_eq!(startup_poll_delay(false), Some(STARTUP_PTY_POLL_INTERVAL));
        assert_eq!(startup_poll_delay(true), None);
    }

    #[test]
    fn startup_trace_title_is_human_readable_without_console_output() {
        assert_eq!(startup_scale_title(1.5), "BetterTerminal M0-beta · 1.5x");
        assert_eq!(
            startup_trace_title(
                Duration::from_millis(682),
                Duration::from_millis(1089),
                1.25,
            ),
            "BetterTerminal M0-beta — bg 682ms · text 1089ms · 1.25x"
        );
    }

    /// PIN — C25: a tab's name is the topmost layer with something to say.
    /// 手动 > 程序标题 (OSC 2) > cwd 叶名 (OSC 7) > profile (mock-up line 2593).
    ///
    /// Red gate: the name used to be `window_title().unwrap_or(profile)`, which
    /// had no manual layer at all and fell from OSC 2 straight past the shell's
    /// own report to the profile.
    #[test]
    fn a_tab_name_takes_the_most_specific_layer_that_actually_spoke() {
        let cwd = Path::new(r"D:\Developer\BetterTerminal");
        assert_eq!(
            display_title(Some("我的构建"), Some("pwsh"), Some(cwd)),
            "我的构建",
            "what you typed outranks everything under it"
        );
        assert_eq!(
            display_title(None, Some("Claude ✳ 任务"), Some(cwd)),
            "Claude ✳ 任务",
            "then what the program announced"
        );
        assert_eq!(
            display_title(None, None, Some(cwd)),
            "BetterTerminal",
            "then where the shell says it is standing"
        );
        assert_eq!(
            display_title(None, None, None),
            "PowerShell",
            "and the profile catches what is left"
        );
    }

    /// M140: the tab's tip states which layer named it, and the answer comes
    /// from the *same* walk that chose the name — including the sanitiser's
    /// fall-through, which a second copy of the precedence rules would lose.
    #[test]
    fn the_tip_names_the_layer_that_actually_named_the_tab() {
        let cwd = Path::new(r"D:\Developer\BetterTerminal");
        assert_eq!(
            resolve_title(Some("build"), Some("pwsh"), Some(cwd)).1,
            Some(tooltip::NameSource::Manual)
        );
        assert_eq!(
            resolve_title(None, Some("pwsh"), Some(cwd)).1,
            Some(tooltip::NameSource::Program)
        );
        assert_eq!(
            resolve_title(None, None, Some(cwd)).1,
            Some(tooltip::NameSource::Cwd)
        );
        // The profile's own name is nobody's claim, so there is no provenance to
        // report and the tip says only the name.
        assert_eq!(resolve_title(None, None, None).1, None);
        // A hostile layer that sanitises away does not get the credit for the
        // layer beneath it: it said nothing, so it named nothing.
        assert_eq!(
            resolve_title(Some("\u{7}"), Some("pwsh"), Some(cwd)),
            ("pwsh".to_owned(), Some(tooltip::NameSource::Program))
        );
    }

    /// The whole second line, assembled — M140's format, F46's extra line, and
    /// the full path rather than the leaf the first line already carries.
    #[test]
    fn a_tabs_tip_is_the_name_then_its_provenance_then_its_promise() {
        let cwd = Path::new(r"D:\Developer\BetterTerminal");
        let (name, source) = resolve_title(None, None, Some(cwd));
        let path = cwd.to_string_lossy().into_owned();
        assert_eq!(
            tooltip::tab_tip(&name, source, Some(&path), false),
            format!("BetterTerminal\nWorking folder · {path}")
        );
        assert_eq!(
            tooltip::tab_tip(&name, source, Some(&path), true),
            format!("BetterTerminal\nWorking folder · {path}\nPinned — restored next launch")
        );
        // The full path, not the leaf the first line already carries.
        assert!(path.ends_with(r"Developer\BetterTerminal"));
    }

    /// I87: the `+` names the profile it would start, so the button says what it
    /// will do rather than merely that it will do something.
    #[test]
    fn the_new_tab_button_names_the_profile_it_would_start() {
        assert_eq!(
            format!(
                "New tab ({})",
                profiles::PROFILES[profiles::DEFAULT_PROFILE].title
            ),
            "New tab (PowerShell)"
        );
    }

    /// The caption run's four boxes map to four tooltip anchors and nothing else
    /// does — a divider is a click target nobody hovers for an explanation.
    #[test]
    fn only_the_caption_run_carries_a_window_chrome_tooltip() {
        assert_eq!(
            tooltip_anchor_for(seats::ChromeTarget::Settings),
            Some(tooltip::TooltipAnchorId::Settings)
        );
        assert_eq!(
            tooltip_anchor_for(seats::ChromeTarget::CloseWindow),
            Some(tooltip::TooltipAnchorId::CloseWindow)
        );
        assert_eq!(tooltip_anchor_for(seats::ChromeTarget::Tab(0)), None);
        assert_eq!(tooltip_anchor_for(seats::ChromeTarget::TabClose(0)), None);
    }

    /// PIN — C25: `cwdLeaf` (mock-up line 2585) is a walk over the path's own
    /// text, which is why a drive root keeps a name where `Path::file_name`
    /// gives none.
    #[test]
    fn the_cwd_layer_names_the_folder_you_are_standing_in() {
        for (path, leaf) in [
            (r"D:\Developer\BetterTerminal", "BetterTerminal"),
            (r"D:\Developer\BetterTerminal\", "BetterTerminal"),
            (r"D:\Developer\BetterTerminal\\", "BetterTerminal"),
            (r"C:\", "C:"),
            (r"\\server\share\work", "work"),
            ("/home/weiyi/src", "src"),
        ] {
            assert_eq!(
                display_title(None, None, Some(Path::new(path))),
                leaf,
                "cwd {path}"
            );
        }
    }

    /// PIN — C26: a program-controlled title is untrusted input. It is stripped
    /// of C0 and C1, trimmed, and capped at `TITLE_MAX`, because "in the product
    /// it must also never be able to impersonate chrome" (mock-up line 2601).
    ///
    /// Red gate: OSC 2 text used to reach the strip byte for byte — a title of
    /// `"\u{1b}[2J"` or eighty characters of anything was drawn as given.
    #[test]
    fn a_program_title_is_stripped_and_capped_before_it_reaches_the_strip() {
        assert_eq!(
            display_title(None, Some("a\u{7}b\u{1b}c"), None),
            "abc",
            "C0 goes, including the escape that starts every sequence"
        );
        assert_eq!(
            display_title(None, Some("\u{9b}0m evil"), None),
            "0m evil",
            "and C1 goes, including the single-byte CSI"
        );
        assert_eq!(
            display_title(None, Some("  \tspaced  "), None),
            "spaced",
            "the trim happens after the strip, as `cleanTitle` writes it"
        );
        assert_eq!(
            display_title(None, Some(&"x".repeat(80)), None)
                .chars()
                .count(),
            TITLE_MAX_CHARS,
            "forty characters, and the forty-first is not a title"
        );
        // A layer that sanitises to nothing has said nothing, and falls through
        // — otherwise a program could blank a tab with one control byte.
        assert_eq!(
            display_title(None, Some("\u{1}\u{2}"), Some(Path::new(r"C:\work"))),
            "work"
        );
        assert_eq!(display_title(None, Some(""), None), "PowerShell");
        assert_eq!(
            display_title(Some("hi\u{0}there"), Some("prog"), None),
            "hithere",
            "the name you type goes through the same sieve (mock-up line 5882)"
        );
        assert_eq!(
            display_title(Some("   "), Some("prog"), None),
            "prog",
            "emptying the override reveals the layer underneath"
        );
    }

    /// PIN — N144: `Ctrl+Shift+P` is the command palette's
    /// (`design/ui-mockup.html` line 5988), so the dev-only preview toggle
    /// stands aside to `Ctrl+Alt+Shift+P`. The mock-up's own palette binding
    /// tests `!e.altKey`, so the chord it leaves free is exactly this one and
    /// the two can never collide.
    ///
    /// Red gate: the toggle used to answer to `Ctrl+Shift+P`, which would have
    /// eaten the palette's chord before it was ever built.
    #[test]
    fn the_dev_preview_toggle_leaves_ctrl_shift_p_to_the_command_palette() {
        let lower = Key::Character("p".into());
        let upper = Key::Character("P".into());
        let ctrl_alt_shift = ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SHIFT;
        assert!(
            !is_preview_toggle_shortcut(&lower, ModifiersState::CONTROL | ModifiersState::SHIFT),
            "Ctrl+Shift+P belongs to the command palette"
        );
        assert!(is_preview_toggle_shortcut(&lower, ctrl_alt_shift));
        assert!(
            is_preview_toggle_shortcut(&upper, ctrl_alt_shift),
            "the chord is matched on the character, whatever case the layout produced"
        );
        // A bare Ctrl+P is DLE and must keep reaching the child; so must every
        // near-miss that is not the whole chord.
        assert!(!is_preview_toggle_shortcut(&lower, ModifiersState::CONTROL));
        assert!(!is_preview_toggle_shortcut(
            &lower,
            ModifiersState::CONTROL | ModifiersState::ALT
        ));
        assert!(!is_preview_toggle_shortcut(&lower, ModifiersState::empty()));
    }

    #[test]
    fn startup_metrics_must_match_the_authoritative_win32_scale_factor() {
        assert!(ensure_metrics_match_authoritative_scale(1.5, 1.5).is_ok());
        assert!(ensure_metrics_match_authoritative_scale(1.0, 1.5).is_err());
    }

    #[test]
    fn recorded_swapchain_size_matches_clamped_physical_inner_after_every_reconcile_size() {
        const LIMIT: u32 = 8192;
        for inner_size in [
            PhysicalSize::new(960, 600),
            PhysicalSize::new(1440, 900),
            PhysicalSize::new(1920, 1200),
            PhysicalSize::new(2560, 1440),
        ] {
            assert!(swapchain_size_matches_inner(
                (inner_size.width, inner_size.height),
                inner_size,
                LIMIT,
            ));
        }
        assert!(swapchain_size_matches_inner(
            (534, LIMIT),
            PhysicalSize::new(534, 65_464),
            LIMIT,
        ));
        assert!(!swapchain_size_matches_inner(
            (3840, 2160),
            PhysicalSize::new(1920, 1200),
            LIMIT,
        ));
    }

    #[test]
    fn pty_pixel_size_is_clamped_to_backend_width() {
        let size = pty_size(
            GridSize {
                columns: std::num::NonZeroU16::new(80).unwrap(),
                rows: std::num::NonZeroU16::new(24).unwrap(),
            },
            PhysicalSize::new(100_000, 80_000),
        );
        assert_eq!((size.pixel_width, size.pixel_height), (u16::MAX, u16::MAX));
    }

    #[test]
    fn private_resize_repaint_input_is_exact_and_integration_gated() {
        assert_eq!(
            psreadline_resize_repaint_input(true),
            Some(PSREADLINE_INVOKE_PROMPT_INPUT)
        );
        assert_eq!(
            psreadline_resize_repaint_input(false),
            None,
            "a session without an open OSC 133 input region injects zero bytes"
        );
    }

    #[test]
    fn resize_storm_reanchor_debt_is_replaced_and_paid_once() {
        let mut pending = false;
        for _ in 0..3 {
            replace_psreadline_resize_reanchor_debt(&mut pending, true);
        }
        assert_eq!(
            take_psreadline_resize_reanchor_input(&mut pending, true),
            Some(PSREADLINE_INVOKE_PROMPT_INPUT),
            "three commits in one open-input transaction coalesce to one chord"
        );
        assert_eq!(
            take_psreadline_resize_reanchor_input(&mut pending, true),
            None,
            "the repair debt is one shot"
        );

        replace_psreadline_resize_reanchor_debt(&mut pending, true);
        replace_psreadline_resize_reanchor_debt(&mut pending, false);
        assert_eq!(
            take_psreadline_resize_reanchor_input(&mut pending, true),
            None,
            "a later closed-region commit replaces stale open-prompt debt"
        );
        replace_psreadline_resize_reanchor_debt(&mut pending, true);
        assert_eq!(
            take_psreadline_resize_reanchor_input(&mut pending, false),
            None,
            "a prompt that closes before quiescence receives no stale chord"
        );
    }

    #[test]
    fn window_resize_coalescer_keeps_only_the_last_size_and_resets_quiet_deadline() {
        let start = Instant::now();
        let first = GridSize {
            columns: std::num::NonZeroU16::new(80).unwrap(),
            rows: std::num::NonZeroU16::new(24).unwrap(),
        };
        let final_grid = GridSize {
            columns: std::num::NonZeroU16::new(112).unwrap(),
            rows: std::num::NonZeroU16::new(31).unwrap(),
        };
        let mut pending = None;
        coalesce_pty_resize(&mut pending, first, PhysicalSize::new(960, 600), start);
        coalesce_pty_resize(
            &mut pending,
            final_grid,
            PhysicalSize::new(1440, 900),
            start + Duration::from_millis(150),
        );

        assert!(take_due_pty_resize(&mut pending, start + Duration::from_millis(349)).is_none());
        let committed =
            take_due_pty_resize(&mut pending, start + Duration::from_millis(350)).unwrap();
        assert_eq!(committed.grid, final_grid);
        assert_eq!(committed.physical, PhysicalSize::new(1440, 900));
        assert!(pending.is_none());
    }

    /// The pure half of `solve_seats` (main.rs's own free function), parameterized on a
    /// `dpi_milli` instead of `&Renderer` so a startup scenario can be solved without a live GPU
    /// device. Numerically identical to what `Runtime::create` and `resolve_seat_layout` do with
    /// an actual renderer in hand — see `solve_seats`'s own body.
    fn solved_terminal_seat(
        seats: &seats::Seats,
        dpi_milli: u32,
        render_physical: PhysicalSize<u32>,
    ) -> bt_render::SeatViewport {
        let metrics = seats::seat_metrics(dpi_milli);
        let viewport = seats::logical_viewport(
            render_physical.width,
            render_physical.height,
            seats::scale_ppm(dpi_milli),
        );
        let layout = match seats.solve(viewport, &metrics) {
            Ok(layout) => layout,
            Err(_) => seats::fit_what_fits(seats, viewport, &metrics).0,
        };
        seats::pane_body_viewport(seats, &layout, seats.terminal(), dpi_milli as f32 / 1_000.0)
            .unwrap_or(bt_render::SeatViewport::whole(
                render_physical.width.max(1),
                render_physical.height.max(1),
            ))
    }

    /// Crossing the one/two-pane boundary changes terminal rows immediately,
    /// while each resulting ConPTY size still waits on the shared 200ms quiet
    /// window used by ordinary window and divider resizes.
    #[test]
    fn pane_count_boundary_reflows_rows_through_the_existing_resize_coalescer() {
        let dpi_milli = 1_000;
        let physical = PhysicalSize::new(1600, 900);
        let metrics = seats::seat_metrics(dpi_milli);
        let mut seats = seats::Seats::lone_terminal();
        let lone = solved_terminal_seat(&seats, dpi_milli, physical);

        assert!(seats.toggle_preview(&metrics));
        let split = solved_terminal_seat(&seats, dpi_milli, physical);
        assert_eq!(lone.y, 40);
        assert_eq!(lone.height, 860, "lone terminal body is the whole seat");
        assert_eq!(split.y, lone.y + 28);
        assert_eq!(split.height, lone.height - 28);

        // Representative renderer metrics make the viewport-to-grid boundary
        // explicit here; CellMetrics::grid_for_pixels owns the same floor.
        let rows_for = |height: u32| ((height.saturating_sub(16)) / 20).max(1) as u16;
        let lone_grid = grid_of(100, rows_for(lone.height));
        let split_grid = grid_of(100, rows_for(split.height));
        assert!(split_grid.rows < lone_grid.rows);

        let start = Instant::now();
        let mut pending = None;
        assert!(coalesce_pty_resize_on_grid_change(
            &mut pending,
            split_grid,
            lone_grid,
            PhysicalSize::new(split.width, split.height),
            start,
        ));
        assert!(
            take_due_pty_resize(
                &mut pending,
                start + WINDOW_RESIZE_QUIET - Duration::from_millis(1)
            )
            .is_none()
        );
        assert_eq!(
            take_due_pty_resize(&mut pending, start + WINDOW_RESIZE_QUIET)
                .unwrap()
                .grid,
            split_grid
        );

        assert!(seats.toggle_preview(&metrics));
        let closed = solved_terminal_seat(&seats, dpi_milli, physical);
        assert_eq!(closed, lone);
        let close_at = start + Duration::from_secs(1);
        assert!(coalesce_pty_resize_on_grid_change(
            &mut pending,
            lone_grid,
            split_grid,
            PhysicalSize::new(closed.width, closed.height),
            close_at,
        ));
        assert_eq!(
            take_due_pty_resize(&mut pending, close_at + WINDOW_RESIZE_QUIET)
                .unwrap()
                .grid,
            lone_grid
        );
    }

    /// PIN (startup order): a session restore with a preview seat open must spawn ConPTY at the
    /// seat's own grid and ask it for nothing more.
    ///
    /// `Runtime::create` resolves the seat layout (`seats::Seats::from_persisted` -> `solve_seats`)
    /// *before* `PtySession::spawn_default`, and seeds `self.grid` to that same solve's grid. §4.2
    /// says solve is pure, so the very first re-solve after `ShowWindow`
    /// (`reconcile_authoritative_dpi`) — run against the identical tree and an identical,
    /// same-DPI viewport — reproduces the identical seat rectangle, and therefore the identical
    /// `GridSize`. `coalesce_pty_resize_on_grid_change` is the single point every later solve
    /// (a live OS `Resized`, a divider drag, a DPI reconciliation) funnels through; fed the exact
    /// pair a clean restore produces, it must schedule nothing.
    #[test]
    fn a_restored_split_tree_reaches_zero_pty_resize_requests_after_a_matching_dpi_spawn() {
        // The shape a preview-narrowed terminal round-trips to `session.json` as: a row split,
        // the terminal first, a pinned preview second (`seats.rs`'s `LeafNodeV1::Preview` docs).
        let node = bt_persist::LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 700_000,
            children: [
                Box::new(bt_persist::LayoutNodeV1::Leaf(
                    bt_persist::LeafNodeV1::Term(bt_persist::TermLeafV1 {
                        profile_id: "pwsh.exe".to_owned(),
                        cwd: String::new(),
                        manual_name: None,
                    }),
                )),
                Box::new(bt_persist::LayoutNodeV1::Leaf(
                    bt_persist::LeafNodeV1::Preview(bt_persist::PreviewLeafV1 { pinned: true }),
                )),
            ],
        });
        let seats = seats::Seats::from_persisted(&node).expect("the tree carries a terminal leaf");
        let dpi_milli = 1_000_u32; // the restored session's recorded DPI equals the monitor's at show
        let render_physical = PhysicalSize::new(1600, 900);

        // The spawn-time solve (before `PtySession::spawn_default`) and the post-`ShowWindow`
        // re-solve, run back to back exactly as startup does.
        let spawn_rect = solved_terminal_seat(&seats, dpi_milli, render_physical);
        let resolved_rect = solved_terminal_seat(&seats, dpi_milli, render_physical);
        assert_eq!(
            spawn_rect, resolved_rect,
            "an unchanged tree against an unchanged viewport must solve to the same seat twice"
        );
        assert!(
            spawn_rect.width < render_physical.width,
            "the preview seat must actually narrow the terminal, or this pin proves nothing"
        );

        // `CellMetrics::grid_for_pixels` is a pure function of the seat rectangle and the
        // (unchanged) font metrics, so an identical rectangle answers an identical `GridSize` on
        // both solves — that arithmetic is already pinned in `bt-render`. What this test pins is
        // the gate downstream of it, fed the one pair of grids a clean restore ever produces.
        let seat_grid = GridSize {
            columns: std::num::NonZeroU16::new(100).unwrap(),
            rows: std::num::NonZeroU16::new(30).unwrap(),
        };
        let physical = PhysicalSize::new(spawn_rect.width, spawn_rect.height);

        // `Runtime::create` seeds `self.grid` to exactly the spawn-time grid; the first
        // post-show solve compares against that same value.
        let current_grid = seat_grid;
        let mut pending = None;
        let now = Instant::now();
        let scheduled = coalesce_pty_resize_on_grid_change(
            &mut pending,
            seat_grid,
            current_grid,
            physical,
            now,
        );
        assert!(
            !scheduled,
            "the first post-spawn solve answers the exact grid the PTY was spawned with"
        );
        assert!(
            take_due_pty_resize(&mut pending, now + WINDOW_RESIZE_QUIET).is_none(),
            "spawn size already equals the seat grid; a matching-DPI restore must schedule zero \
             ConPTY resizes"
        );
    }

    /// RED-CHECK for the pin above: proves it is not vacuous. The pre-fix `resize()` and
    /// `commit_seat_geometry()` called `coalesce_pty_resize` unconditionally — the old
    /// spawn-then-resize shape this pin exists to forbid — which schedules a real ConPTY resize
    /// even when the grid the PTY was spawned with never moved. Restoring that unconditional call
    /// at the two real call sites is exactly what turns the pin above red.
    #[test]
    fn the_old_unconditional_coalesce_would_have_scheduled_a_resize_for_an_unchanged_grid() {
        let grid = GridSize {
            columns: std::num::NonZeroU16::new(100).unwrap(),
            rows: std::num::NonZeroU16::new(30).unwrap(),
        };
        let mut pending = None;
        let now = Instant::now();
        // The old shape: no `next_grid != current_grid` gate at all.
        coalesce_pty_resize(&mut pending, grid, PhysicalSize::new(1000, 700), now);
        assert!(
            take_due_pty_resize(&mut pending, now + WINDOW_RESIZE_QUIET).is_some(),
            "an unconditional coalesce call schedules a resize even for an unchanged grid"
        );
    }

    /// The scheduling half of `Runtime`, with no GPU in it.
    ///
    /// Every decision below is taken by the *production* functions — `plan_grid_change`,
    /// `release_due_pty_resize`, `pty_resize_wake_deadline` — and the gate answer comes from a real
    /// `DualPlaneSession` fed real OSC 133 bytes, not from a bool a test invented. What the harness
    /// itself owns is only the bookkeeping the runtime does around them: applying the reflow to the
    /// session, and recording the ConPTY requests that were actually issued.
    struct ResizeGateHarness {
        session: DualPlaneSession,
        pending: Option<PendingPtyResize>,
        grid: GridSize,
        conpty: GridSize,
        requests: Vec<GridSize>,
        typed_input_resize_deferral: bool,
    }

    impl ResizeGateHarness {
        fn new(columns: u16, rows: u16) -> Self {
            Self::with_policy(columns, rows, TYPED_INPUT_RESIZE_DEFERRAL)
        }

        fn with_policy(columns: u16, rows: u16, typed_input_resize_deferral: bool) -> Self {
            let grid = grid_of(columns, rows);
            Self {
                session: DualPlaneSession::new(
                    NonZeroU32::from(grid.columns),
                    NonZeroU32::from(grid.rows),
                ),
                pending: None,
                grid,
                conpty: grid,
                requests: Vec::new(),
                typed_input_resize_deferral,
            }
        }

        fn deferring(&self) -> bool {
            typed_input_resize_deferral_active(
                self.typed_input_resize_deferral,
                true,
                self.session.typed_shell_input_live(),
            )
        }

        fn feed(&mut self, bytes: &[u8], at: Instant) {
            self.session.feed_at(bytes, at).unwrap();
        }

        /// One `WindowEvent::Resized` worth of work: the seat has already moved, the solve has
        /// answered `columns`, and this is everything `Runtime::resize` does with that answer.
        fn drag_to(&mut self, columns: u16, at: Instant) {
            let next = grid_of(columns, self.grid.rows.get());
            let deferred = self.deferring();
            if let Some(reflow) = plan_grid_change(
                &mut self.pending,
                next,
                self.conpty,
                self.grid,
                PhysicalSize::new(u32::from(columns) * 8, 600),
                at,
                deferred,
            ) {
                self.session
                    .resize(
                        NonZeroU32::from(reflow.columns),
                        NonZeroU32::from(reflow.rows),
                    )
                    .unwrap();
                self.grid = reflow;
            }
        }

        /// One `about_to_wait`: drain has already happened, so the gate is asked again here.
        fn tick(&mut self, at: Instant) {
            let deferred = self.deferring();
            let (pending, _) = service_pending_pty_resize(&mut self.pending, at, deferred);
            let Some(pending) = pending else {
                return;
            };
            if pending.grid != self.grid {
                self.session
                    .resize(
                        NonZeroU32::from(pending.grid.columns),
                        NonZeroU32::from(pending.grid.rows),
                    )
                    .unwrap();
                self.grid = pending.grid;
            }
            self.conpty = pending.grid;
            self.requests.push(pending.grid);
        }

        fn wake_deadline(&self, now: Instant) -> Option<Instant> {
            pty_resize_wake_deadline(self.pending, self.deferring(), now)
        }
    }

    fn grid_of(columns: u16, rows: u16) -> GridSize {
        GridSize {
            columns: std::num::NonZeroU16::new(columns).unwrap(),
            rows: std::num::NonZeroU16::new(rows).unwrap(),
        }
    }

    /// POLICY PIN (user ruling 2026-08-06): typed input does not hold a resize by default. The
    /// local grid follows the drag immediately, while ConPTY still receives exactly the coalesced
    /// final size at the ordinary quiet boundary.
    #[test]
    fn typed_input_resize_deferral_is_off_and_live_resize_is_the_default() {
        const {
            assert!(!TYPED_INPUT_RESIZE_DEFERRAL);
        }
        assert!(!typed_input_resize_deferral_active(false, true, true));
        assert!(typed_input_resize_deferral_active(true, true, true));
        assert!(!typed_input_resize_deferral_active(true, false, true));
        assert!(!typed_input_resize_deferral_active(true, true, false));

        let start = Instant::now();
        let mut harness = ResizeGateHarness::new(100, 24);
        harness.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07Get-ChildItem", start);
        assert!(harness.session.typed_shell_input_live());

        harness.drag_to(80, start);
        assert_eq!(
            harness.grid,
            grid_of(80, 24),
            "policy=false must reflow locally even while the shell holds text"
        );
        harness.tick(start + WINDOW_RESIZE_QUIET - Duration::from_millis(1));
        assert!(
            harness.requests.is_empty(),
            "the 200 ms coalescer remains in force"
        );
        harness.tick(start + WINDOW_RESIZE_QUIET);
        assert_eq!(
            harness.requests,
            vec![grid_of(80, 24)],
            "policy=false commits the coalesced resize while input remains live"
        );
    }

    /// PIN: a PTY deadline handed to `ControlFlow::WaitUntil` must still be in the future.
    ///
    /// The gate can be sampled as live while servicing the pending resize and read empty by the
    /// later control-flow calculation. In that state `blank_since` is still `None`, so the pending
    /// request's coalescing deadline is the only deadline available -- and it is already due. A
    /// past `WaitUntil` makes winit immediately re-enter `about_to_wait` instead of sleeping.
    #[test]
    fn a_gate_transition_cannot_offer_wait_until_an_already_due_resize_deadline() {
        let start = Instant::now();
        let mut pending = None;
        coalesce_pty_resize(
            &mut pending,
            grid_of(80, 24),
            PhysicalSize::new(800, 600),
            start,
        );
        let now = start + WINDOW_RESIZE_QUIET;

        assert!(
            pty_resize_wake_deadline(pending, false, now).is_none_or(|deadline| deadline > now),
            "ControlFlow::WaitUntil must never receive an already-due PTY deadline"
        );
        let (released, wake_deadline) = service_pending_pty_resize(&mut pending, now, false);
        assert!(released.is_none(), "one empty sample cannot release");
        assert_eq!(
            wake_deadline,
            Some(now + WINDOW_RESIZE_QUIET),
            "the same gate sample starts confirmation and schedules its future boundary"
        );
    }

    /// MACHINERY PIN (policy=true): while an OSC 133 input region holds typed content, a PTY
    /// resize is deferred — zero requests during the drag, exactly one at release carrying the
    /// final dragged size, and our own grid held in lockstep with the child's the whole way.
    ///
    /// This is the minimal repro's shape, in the app's own scheduling terms: type without
    /// submitting, drag narrower than the prompt, drag wider again, and only then let go.
    #[test]
    fn a_drag_while_the_shell_holds_typed_input_reaches_conpty_exactly_once_at_release() {
        let start = Instant::now();
        let mut harness = ResizeGateHarness::with_policy(100, 24, true);
        harness.feed(
            b"\x1b]133;A\x07PS D:\\Developer\\BetterTerminal> \x1b]133;B\x07Get-ChildItem",
            start,
        );
        assert!(harness.session.typed_shell_input_live());

        // The narrowing drag, then the widening one, with the loop turning between each step.
        for (step, columns) in [92_u16, 84, 70, 39, 55, 80, 96].into_iter().enumerate() {
            let at = start + Duration::from_millis(20 * step as u64);
            harness.drag_to(columns, at);
            harness.tick(at + Duration::from_millis(1));
        }
        // Long past every quiet deadline the drag ever set: the gate is not a debounce.
        harness.tick(start + Duration::from_secs(3600));

        assert!(
            harness.requests.is_empty(),
            "zero ConPTY resizes are owed while the child is holding text, got {:?}",
            harness.requests
        );
        assert_eq!(
            (harness.grid, harness.conpty),
            (grid_of(100, 24), grid_of(100, 24)),
            "our grid and the child's stay in lockstep at the old width across the whole drag"
        );
        assert!(
            harness
                .wake_deadline(start + Duration::from_secs(3600))
                .is_none(),
            "a held resize must not offer the loop a deadline it would spin on"
        );

        // RELEASE: the command is submitted, and the empty line that follows it is confirmed over
        // one quiet window (`a_redraw_split_across_two_reads_never_releases_in_its_blank_window` is
        // why the drain that carries `133;C` cannot be trusted on its own). It carries the last size
        // the drag reached — not the narrow one it passed through, and not one request per step.
        let released_at = start + Duration::from_secs(3600);
        harness.feed(b"\r\n\x1b]133;C\x07", released_at);
        harness.tick(released_at);
        assert!(harness.requests.is_empty());
        assert_eq!(
            harness.wake_deadline(released_at),
            Some(released_at + WINDOW_RESIZE_QUIET),
            "the loop is told when to come back and confirm, since no further output need arrive"
        );
        harness.tick(released_at + WINDOW_RESIZE_QUIET);
        assert_eq!(
            harness.requests,
            vec![grid_of(96, 24)],
            "exactly one resize lands at release, carrying the final dragged size"
        );
        assert_eq!(harness.grid, grid_of(96, 24));
        assert_eq!(harness.conpty, grid_of(96, 24));
    }

    /// MACHINERY PIN (policy=true): the three non-holding/release cases stay cheap.
    #[test]
    fn an_idle_prompt_a_bare_screen_and_a_cleared_line_all_resize_without_deferral() {
        let start = Instant::now();

        // An empty prompt: the drag lands as it always has. The confirmation window a release now
        // waits out started at this drag's own first frame, so it has already elapsed when the
        // coalescer's deadline arrives — the deadline the loop is given is the coalescer's own.
        let mut idle = ResizeGateHarness::with_policy(100, 24, true);
        idle.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07", start);
        idle.drag_to(80, start);
        assert_eq!(
            idle.grid,
            grid_of(80, 24),
            "an empty prompt reflows immediately"
        );
        assert_eq!(
            idle.wake_deadline(start),
            Some(start + WINDOW_RESIZE_QUIET),
            "confirming an already-idle prompt must not add a second quiet window to the drag"
        );
        idle.tick(start + WINDOW_RESIZE_QUIET);
        assert_eq!(idle.requests, vec![grid_of(80, 24)]);

        // A screen that never emitted OSC 133 is untouched by any of this.
        let mut bare = ResizeGateHarness::with_policy(100, 24, true);
        bare.feed(b"PS> Get-ChildItem", start);
        bare.drag_to(80, start);
        assert_eq!(bare.grid, grid_of(80, 24));
        bare.tick(start + WINDOW_RESIZE_QUIET);
        assert_eq!(
            bare.requests,
            vec![grid_of(80, 24)],
            "honest degradation: without shell integration this is today's path, unchanged"
        );

        // Clearing the line is a release in its own right — no submission needed. What it does need
        // is the confirmation window, because "the line is empty" is exactly what a redraw's erase
        // chunk also says one millisecond before its rewrite arrives.
        let mut cleared = ResizeGateHarness::with_policy(100, 24, true);
        cleared.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07Get-ChildItem", start);
        cleared.drag_to(80, start);
        cleared.tick(start + WINDOW_RESIZE_QUIET);
        assert!(cleared.requests.is_empty());
        let emptied_at = start + WINDOW_RESIZE_QUIET;
        cleared.feed(b"\x1b[5G\x1b[K", emptied_at);
        cleared.tick(emptied_at);
        assert!(
            cleared.requests.is_empty(),
            "the first blank sample only starts the confirmation"
        );
        cleared.tick(emptied_at + WINDOW_RESIZE_QUIET);
        assert_eq!(
            cleared.requests,
            vec![grid_of(80, 24)],
            "a line that stays empty releases the gate with no submission"
        );
    }

    /// MACHINERY PIN (policy=true): a drag that wanders away and comes back tells the child
    /// nothing at all. Without this,
    /// "exactly one resize at release" would be false whenever the final size is the one the child
    /// never left — and that resize would be a gratuitous reflow of a settled prompt.
    #[test]
    fn a_drag_that_returns_to_the_childs_own_size_releases_nothing() {
        let start = Instant::now();
        let mut harness = ResizeGateHarness::with_policy(100, 24, true);
        harness.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07Get-ChildItem", start);
        for columns in [70_u16, 39, 100] {
            harness.drag_to(columns, start);
        }
        harness.feed(b"\r\n\x1b]133;C\x07", start);
        harness.tick(start + WINDOW_RESIZE_QUIET);
        assert!(
            harness.requests.is_empty(),
            "the child is already 100 columns wide; nothing is owed"
        );
        assert_eq!(harness.grid, grid_of(100, 24));
    }

    /// MACHINERY PIN (policy=true, confirm-then-release): the blank instant inside a redraw is not
    /// an empty prompt.
    ///
    /// The reader thread hands the loop whatever a single read returned — up to 16 KiB — and wakes
    /// it for every chunk. PSReadLine redraws a line by parking the cursor on `B`, erasing, and
    /// writing the buffer back, so a redraw that straddles two reads leaves a wake in between where
    /// the grid honestly holds nothing. Sampling the gate once at that wake and releasing on it is
    /// how the narrow width reached the child through a gate with no bypass in it.
    ///
    /// So a release needs the *same* answer for `WINDOW_RESIZE_QUIET` — the quiet period already
    /// used for "the drag has stopped" — and any sample that reads content restarts the clock. The
    /// millisecond-scale blank of a redraw can never clear that bar; a line the user actually
    /// emptied always does.
    #[test]
    fn a_redraw_split_across_two_reads_never_releases_in_its_blank_window() {
        let start = Instant::now();
        let mut harness = ResizeGateHarness::with_policy(100, 24, true);
        // A 12-column prompt: `B` lands on column 12, which is CUP column 13.
        harness.feed(
            b"\x1b]133;A\x07PS D:\\dist> \x1b]133;B\x07Get-ChildItem",
            start,
        );
        assert!(harness.session.typed_shell_input_live());

        harness.drag_to(35, start);
        harness.tick(start + Duration::from_millis(1));
        assert!(
            harness.requests.is_empty(),
            "the buffer is full; nothing is owed"
        );

        // The chunk that carries the erase but not the rewrite, drained at a wake past the
        // coalescer's own deadline.
        let erased_at = start + WINDOW_RESIZE_QUIET + Duration::from_millis(5);
        harness.feed(b"\x1b[13G\x1b[J", erased_at);
        assert!(
            !harness.session.typed_shell_input_live(),
            "the fixture must really empty the grid, or this pin proves nothing"
        );
        harness.tick(erased_at);
        assert!(
            harness.requests.is_empty(),
            "one blank sample is not an empty prompt: {:?}",
            harness.requests
        );

        // The rewrite lands three milliseconds later. The buffer was never empty.
        let rewritten_at = erased_at + Duration::from_millis(3);
        harness.feed(b"Get-ChildItem2", rewritten_at);
        harness.tick(rewritten_at);
        assert!(harness.requests.is_empty());

        // A second redraw's erase, ten milliseconds after the first. The confirmation window must
        // restart here, not run from the first blank sample.
        let erased_again_at = erased_at + Duration::from_millis(10);
        harness.feed(b"\x1b[13G\x1b[J", erased_again_at);
        harness.tick(erased_again_at);
        harness.tick(erased_at + WINDOW_RESIZE_QUIET);
        assert!(
            harness.requests.is_empty(),
            "a content sample between the two blanks restarts the confirmation: {:?}",
            harness.requests
        );

        harness.feed(b"Get-ChildItem23", erased_at + WINDOW_RESIZE_QUIET);
        harness.tick(erased_again_at + WINDOW_RESIZE_QUIET);
        assert!(harness.requests.is_empty());

        // RELEASE: the command is submitted, and the line stays empty through the window.
        let submitted_at = start + Duration::from_secs(1);
        harness.feed(b"\r\n\x1b]133;C\x07", submitted_at);
        harness.tick(submitted_at);
        assert!(
            harness.requests.is_empty(),
            "even a submission is confirmed before it releases"
        );
        harness.tick(submitted_at + WINDOW_RESIZE_QUIET);
        assert_eq!(
            harness.requests,
            vec![grid_of(35, 24)],
            "exactly one resize, carrying the dragged size, once the line is confirmed empty"
        );
    }

    /// RED-CHECK for the deferral pin: prove it is not vacuous. This is the pre-mitigation path —
    /// the same drag with the gate answer forced to `false` — and it is exactly what the forensic
    /// run showed reaching PSReadLine: the 39-column width, narrower than the prompt, delivered
    /// while the input buffer is non-empty.
    #[test]
    fn without_the_gate_the_same_drag_hands_conpty_the_narrow_width_mid_input() {
        let start = Instant::now();
        let mut pending = None;
        let mut conpty = grid_of(100, 24);
        let mut local = conpty;
        let mut requests = Vec::new();
        for (step, columns) in [92_u16, 84, 70, 39, 55, 80, 96].into_iter().enumerate() {
            let at = start + Duration::from_millis(20 * step as u64);
            let next = grid_of(columns, 24);
            // `typed_input_live: false` is the clause removed.
            if let Some(reflow) = plan_grid_change(
                &mut pending,
                next,
                conpty,
                local,
                PhysicalSize::new(u32::from(columns) * 8, 600),
                at,
                false,
            ) {
                local = reflow;
            }
            if let Some(due) = release_due_pty_resize(&mut pending, at + WINDOW_RESIZE_QUIET, false)
            {
                conpty = due.grid;
                requests.push(due.grid);
            }
        }
        assert!(
            requests.contains(&grid_of(39, 24)),
            "the ungated path really does hand the child a width narrower than the prompt, \
             which is the whole precondition of the PSReadLine anchor fault: {requests:?}"
        );
    }

    #[test]
    fn resize_atomic_present_gate_rejects_the_previous_grid_frame() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
        let mut projection = session.new_projection(session.layout_key());
        let old_frame = session.viewport_frame(&mut projection).unwrap();
        let new_grid = GridSize {
            columns: std::num::NonZeroU16::new(42).unwrap(),
            rows: std::num::NonZeroU16::new(12).unwrap(),
        };
        assert!(!frame_matches_grid(&old_frame, new_grid));

        session
            .resize(NonZeroU32::new(42).unwrap(), NonZeroU32::new(12).unwrap())
            .unwrap();
        session.refresh_projection(&mut projection);
        let new_frame = session.viewport_frame(&mut projection).unwrap();
        assert!(frame_matches_grid(&new_frame, new_grid));
    }

    #[test]
    fn pty_mode_only_update_is_presentation_equivalent_but_text_is_not() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(3).unwrap());
        let mut projection = session.new_projection(session.layout_key());
        let before = session.viewport_frame(&mut projection).unwrap();

        session.feed(b"\x1b[?2004h").unwrap();
        session.refresh_projection(&mut projection);
        let mode_only = session.viewport_frame(&mut projection).unwrap();
        assert!(presentation_equivalent(&before, &mode_only));

        session.feed(b"visible").unwrap();
        session.refresh_projection(&mut projection);
        let text = session.viewport_frame(&mut projection).unwrap();
        assert!(!presentation_equivalent(&mode_only, &text));
    }

    #[test]
    fn content_before_trailing_bsu_is_published_and_survives_empty_sync_timeout() {
        let mut harness = PtyPresentationHarness::new(20, 2);

        assert!(harness.feed_drain(b"visible-before-bsu\x1b[?2026h"));
        assert!(harness.session.synchronized_update_deadline().is_some());
        assert_eq!(harness.publications, 1);
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                .contains("visible-before-bsu")
        );

        let (finished, republished) = harness.finish_synchronized_update();
        assert!(finished);
        assert!(
            !republished,
            "the already-pending visible frame is equivalent"
        );
        assert_eq!(harness.publications, 1);
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                .contains("visible-before-bsu")
        );
    }

    #[test]
    fn completed_sync_update_is_published_before_a_trailing_bsu_in_the_same_drain() {
        let mut harness = PtyPresentationHarness::new(24, 2);

        assert!(harness.feed_drain(b"\x1b[?2026h\x1b[H\x1b[2Kclosed-update\x1b[?2026l\x1b[?2026h"));
        assert!(harness.session.synchronized_update_deadline().is_some());
        assert_eq!(harness.publications, 1);
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("closed-update")
        );
    }

    #[test]
    fn open_synchronized_update_still_suppresses_its_intermediate_state() {
        let mut harness = PtyPresentationHarness::new(24, 2);
        assert!(harness.feed_drain(b"base"));
        assert!(harness.present_pending());

        assert!(!harness.feed_drain(b"\x1b[?2026h\rhidden-intermediate"));
        assert!(harness.session.synchronized_update_deadline().is_some());
        assert_eq!(harness.publications, 1);
        assert!(harness.pending.pending_frame().is_none());
        assert!(frame_row_text(harness.last_presented.as_ref().unwrap(), 0).contains("base"));

        assert!(harness.feed_drain(b"\x1b[?2026l"));
        assert!(harness.session.synchronized_update_deadline().is_none());
        assert_eq!(harness.publications, 2);
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                .contains("hidden-intermediate")
        );
    }

    #[test]
    fn rapid_synchronized_update_chain_never_withholds_a_completed_frame() {
        const UPDATE_COUNT: usize = 81;
        let mut harness = PtyPresentationHarness::new(24, 2);

        for update in 0..UPDATE_COUNT {
            let prefix = if update == 0 { "\x1b[?2026h" } else { "" };
            let bytes = format!("{prefix}\x1b[H\x1b[2Kframe-{update:02}\x1b[?2026l\x1b[?2026h");
            assert!(
                harness.feed_drain(bytes.as_bytes()),
                "completed update {update} was not published"
            );
            assert!(harness.session.synchronized_update_deadline().is_some());
            assert!(
                frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                    .contains(&format!("frame-{update:02}"))
            );
        }

        assert_eq!(harness.publications, UPDATE_COUNT);
        assert!(!harness.feed_drain(b"\x1b[?2026l"));
        assert!(harness.session.synchronized_update_deadline().is_none());
        assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("frame-80"));
        assert!(harness.present_pending());
        assert!(harness.pending.pending_frame().is_none());
    }

    #[test]
    fn pending_frame_is_the_a_b_a_equivalence_baseline() {
        let mut harness = PtyPresentationHarness::new(4, 1);

        assert!(harness.feed_drain(b"A"));
        assert!(harness.present_pending());
        assert!(frame_row_text(harness.last_presented.as_ref().unwrap(), 0).contains('A'));

        assert!(harness.feed_drain(b"\rB"));
        assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains('B'));
        assert!(harness.feed_drain(b"\rA"));
        assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains('A'));
        assert_eq!(harness.publications, 3);
    }

    #[test]
    fn anchored_mouse_forwarding_uses_live_viewport_rows_and_clamps_frozen_rows() {
        assert!(!UserInputKind::Mouse.returns_view_to_live());
        assert!(UserInputKind::Keyboard.returns_view_to_live());

        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
        session
            .feed(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive")
            .unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let bottom_frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(bottom_frame.scroll_offset_rows, 0);

        let physical_hit = bt_render::GridHit { row: 3, column: 5 };
        assert_eq!(
            live_viewport_mouse_hit(&bottom_frame, physical_hit),
            physical_hit
        );

        projection.scroll_by_rows(2);
        session.refresh_projection(&mut projection);
        let anchored_frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(anchored_frame.scroll_offset_rows, 2);

        let live_hit = live_viewport_mouse_hit(&anchored_frame, physical_hit);
        assert_eq!(live_hit, bt_render::GridHit { row: 1, column: 5 });
        assert_eq!(
            input::mouse_bytes(
                true,
                input::MouseProtocolButton::Left,
                input::MouseProtocolEvent::Press,
                live_hit.row,
                live_hit.column,
                ModifiersState::empty(),
            ),
            b"\x1b[<0;6;2M"
        );

        assert_eq!(
            live_viewport_mouse_hit(&anchored_frame, bt_render::GridHit { row: 1, column: 5 },),
            bt_render::GridHit { row: 0, column: 5 }
        );
    }

    #[test]
    fn expanded_presented_row_map_drives_forwarded_mouse_row_and_column() {
        let start = Instant::now();
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(12).unwrap());
        session
            .feed_at(
                b"\x1b[?1049h$$x^2$$\r\nbarrier1\r\nbarrier2\r\nbottom",
                start,
            )
            .unwrap();
        assert_eq!(
            session.advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
            1
        );
        let mut task = session.take_live_worker_task().unwrap();
        let raster = render_live_detection_task(&MathEngine::new(), &mut task, foreground_rgb())
            .expect("test formula rasterizes through the production live worker entry");
        let ink_height_px = raster.height_px;
        assert!(session.complete_live_worker_result(task, Ok(raster)));

        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let cell_height = 18 * bt_viewport::SUBPIXELS_PER_PX;
        assert!(frame.row_map[0].height_subpixels > cell_height);
        assert_eq!(frame.math_blocks[0].artifact.render_scale_milli, 1000);
        let padding = cell_height / 4;
        assert_eq!(
            frame.math_blocks[0].artifact.height_subpixels,
            i64::from(ink_height_px) * bt_viewport::SUBPIXELS_PER_PX + 2 * padding,
            "display box height is alpha-tight ink plus symmetric 25% cell padding"
        );
        assert_eq!(
            frame.math_blocks[0].artifact.vertical_padding_subpixels,
            padding
        );

        let target = frame.row_map[2];
        let target_y = target.top_subpixels + target.height_subpixels / 2;
        let visual_hit = bt_render::GridHit {
            row: frame
                .visual_row_at(target_y)
                .expect("expanded logical row remains hittable in the presented frame"),
            column: 5,
        };
        let forwarded = live_viewport_mouse_hit(&frame, visual_hit);
        assert_eq!(forwarded, bt_render::GridHit { row: 2, column: 5 });
        assert_eq!(
            input::mouse_bytes(
                true,
                input::MouseProtocolButton::Left,
                input::MouseProtocolEvent::Press,
                forwarded.row,
                forwarded.column,
                ModifiersState::empty(),
            ),
            b"\x1b[<0;6;3M"
        );
    }

    #[test]
    fn forwarded_mouse_hit_stays_bound_to_the_presented_frame_during_an_unpresented_shift() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(6).unwrap());
        session
            .feed(b"\x1b[?1003h\x1b[?1006ha\r\nb\r\nc\r\nheader\r\nx\r\ny")
            .unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let presented = session.viewport_frame(&mut projection).unwrap();
        assert!(frame_row_text(&presented, 3).contains("header"));

        session.feed(b"\r\nexpanded-1\r\nexpanded-2").unwrap();
        session.refresh_projection(&mut projection);
        let unpresented = session.viewport_frame(&mut projection).unwrap();
        assert!(frame_row_text(&unpresented, 1).contains("header"));
        assert!(!frame_row_text(&unpresented, 3).contains("header"));

        let stale_aim = bt_render::GridHit { row: 3, column: 0 };
        let forwarded = live_viewport_mouse_hit(&presented, stale_aim);
        let mut route = None;
        let bytes = route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            forwarded,
            session.terminal_modes(),
            ModifiersState::empty(),
        )
        .unwrap();

        assert_eq!(bytes, b"\x1b[<0;1;4M");
        assert_eq!(
            forwarded.row, 3,
            "the stale row correctly misses live row 1"
        );
    }

    #[test]
    fn tracked_tui_mouse_keeps_priority_over_ctrl_link_gesture() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
        session.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
        let hit = bt_render::GridHit { row: 1, column: 2 };
        let mut route = None;
        let forwarded = route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::CONTROL,
        );
        assert!(forwarded.is_some());
        assert!(matches!(route, Some(MouseRoute::Forward(_))));

        let mut shifted_route = None;
        assert!(
            route_forwarded_mouse_button(
                &mut shifted_route,
                ElementState::Pressed,
                input::MouseProtocolButton::Left,
                hit,
                session.terminal_modes(),
                ModifiersState::CONTROL | ModifiersState::SHIFT,
            )
            .is_none()
        );
        assert!(shifted_route.is_none());
    }

    #[test]
    fn stationary_double_click_stays_strictly_paired_across_tui_repaints() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(6).unwrap());
        session.feed(b"\x1b[?1000h\x1b[?1006hready").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let hit = bt_render::GridHit { row: 3, column: 8 };
        let mut route = None;
        let mut captured_user_input = Vec::new();

        for repaint in [
            b"\x1b[Hfirst repaint\r\na\r\nb\r\nc\r\nd\r\ne\r\nf".as_slice(),
            b"\x1b[Hsecond repaint\r\ng\r\nh\r\ni\r\nj\r\nk\r\nl",
        ] {
            let frame = session.viewport_frame(&mut projection).unwrap();
            assert_eq!(frame.scroll_offset_rows, 0);
            let live_hit = live_viewport_mouse_hit(&frame, hit);
            for state in [ElementState::Pressed, ElementState::Released] {
                captured_user_input.push(
                    route_forwarded_mouse_button(
                        &mut route,
                        state,
                        input::MouseProtocolButton::Left,
                        live_hit,
                        session.terminal_modes(),
                        ModifiersState::empty(),
                    )
                    .expect("tracked click must produce one PTY write per edge"),
                );
            }
            assert!(route.is_none());
            session.feed(repaint).unwrap();
            session.refresh_projection(&mut projection);
        }

        assert_eq!(
            captured_user_input,
            [
                b"\x1b[<0;9;4M".to_vec(),
                b"\x1b[<0;9;4m".to_vec(),
                b"\x1b[<0;9;4M".to_vec(),
                b"\x1b[<0;9;4m".to_vec(),
            ]
        );
    }

    #[test]
    fn post_drag_wheel_frame_reaches_the_renderer_text_row_slice_boundary() {
        let start = Instant::now();
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(3).unwrap());
        session.feed_at(b"a\r\nb\r\nc", start).unwrap();
        session
            .resize_at(
                NonZeroU32::new(4).unwrap(),
                NonZeroU32::new(2).unwrap(),
                start + Duration::from_millis(10),
            )
            .unwrap();
        session.mark_pty_resize_requested_at(
            NonZeroU32::new(4).unwrap(),
            NonZeroU32::new(2).unwrap(),
            start + Duration::from_millis(210),
        );
        session
            .feed_at(b"\r\nx", start + Duration::from_millis(220))
            .unwrap();
        assert!(
            session
                .finish_resize_if_quiescent(start + Duration::from_millis(420))
                .unwrap()
        );

        let mut projection = session.new_projection(session.layout_key());
        projection.scroll_by_rows(1);
        session.refresh_projection(&mut projection);
        let frame = session.viewport_frame(&mut projection).unwrap();
        session.record_published_frame(&frame, start + Duration::from_millis(421));
        let render_rows = bt_render::text_row_cells(&frame)
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(frame.grid_rows.get(), 2);
        assert_eq!(frame.rows.get(), 3);
        assert_eq!(frame.drawable_rows(), 2);
        assert_eq!(render_rows.len(), 3);
        assert!(render_rows.iter().all(|row| row.len() == 4));
    }

    #[test]
    fn panic_log_uses_the_process_temp_directory_without_requiring_stderr() {
        assert_eq!(
            panic_log_path(),
            std::env::temp_dir().join("bt-app-panic.log")
        );
    }

    #[test]
    fn one_cell_terminal_and_zero_pixel_transition_are_defended() {
        let one = std::num::NonZeroU16::new(1).unwrap();
        let grid = GridSize {
            columns: one,
            rows: one,
        };
        let backend = pty_size(grid, PhysicalSize::new(0, 0));
        assert_eq!((backend.columns.get(), backend.rows.get()), (1, 1));
        assert_eq!((backend.pixel_width, backend.pixel_height), (0, 0));

        let mut session =
            DualPlaneSession::new(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap());
        session.feed(b"A").unwrap();
        session
            .resize(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap())
            .unwrap();
        assert_eq!(session.terminal().visible_text(), ["A"]);
    }

    #[test]
    fn direct_width_fixture_places_legacy_and_2027_closing_bars_on_their_rulers() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
        session
            .feed(include_bytes!("../../../scripts/dev/width-probe-input.vt"))
            .unwrap();

        let closing_bar = |row: u32, start: usize| {
            session
                .terminal()
                .visible_row(row)
                .unwrap()
                .cells
                .iter()
                .enumerate()
                .skip(start)
                .filter(|(_, cell)| cell.text == "|")
                .map(|(column, _)| column)
                .nth(1)
                .unwrap()
        };
        let rows = [2, 4, 6, 8, 10, 12, 14];
        let legacy_widths = [8, 4, 2, 1, 7, 1, 1];
        let mode_2027_widths = [2, 2, 2, 1, 7, 1, 2];
        for ((row, legacy), clustered) in rows.into_iter().zip(legacy_widths).zip(mode_2027_widths)
        {
            assert_eq!(closing_bar(row, 0), 1 + legacy, "legacy content row {row}");
            assert_eq!(
                closing_bar(row + 1, 0),
                1 + legacy,
                "legacy ruler row {}",
                row + 1
            );
            assert_eq!(
                closing_bar(row, 40),
                41 + clustered,
                "2027 content row {row}"
            );
            assert_eq!(
                closing_bar(row + 1, 40),
                41 + clustered,
                "2027 ruler row {}",
                row + 1
            );
        }
    }

    #[test]
    fn direct_glyph_fixture_preserves_emoji_and_ambiguous_cell_occupancy() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
        session
            .feed(include_bytes!("../../../scripts/dev/glyph-probe-input.vt"))
            .unwrap();

        let bar_columns = |row: u32| {
            session
                .terminal()
                .visible_row(row)
                .unwrap()
                .cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| cell.text == "|")
                .map(|(column, _)| column)
                .collect::<Vec<_>>()
        };
        for (content_row, width) in [(2, 2), (4, 2), (6, 2), (8, 2), (10, 1)] {
            assert_eq!(bar_columns(content_row), [0, 1 + width]);
            assert_eq!(bar_columns(content_row + 1), [0, 1 + width]);
        }
        assert_eq!(bar_columns(13), [0, 2, 4]);
        assert_eq!(bar_columns(14), [0, 2, 4]);
    }

    #[test]
    fn real_powershell_input_reaches_a_viewport_owned_frame() {
        let columns = std::num::NonZeroU16::new(48).unwrap();
        let rows = std::num::NonZeroU16::new(10).unwrap();
        let mut pty =
            PtySession::spawn_default(PtySize::cells(columns, rows), Arc::new(|| {})).unwrap();
        let mut session = DualPlaneSession::with_quotas_and_cell_height(
            nonzero_u32(columns.get()),
            nonzero_u32(rows.get()),
            DEFAULT_STAGING_QUOTA,
            M0_FROZEN_LINE_QUOTA,
            std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut command_sent = false;
        let mut output_seen = false;
        while Instant::now() < deadline {
            let bytes = pty.read_output();
            if !bytes.is_empty() {
                session.feed(&bytes).unwrap();
                let replies = session.take_pty_writes();
                for reply in &replies {
                    pty.write(reply).unwrap();
                }
                if !command_sent && !replies.is_empty() {
                    pty.write(&ime_commit_bytes("Write-Output ('BT_APP_' + 'INPUT_OK')\r"))
                        .unwrap();
                    command_sent = true;
                }
                output_seen = session
                    .terminal()
                    .visible_text()
                    .iter()
                    .any(|line| line.contains("BT_APP_INPUT_OK"));
                if output_seen {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert!(
            command_sent,
            "PowerShell never completed its terminal handshake"
        );
        assert!(output_seen, "PowerShell command output never reached Term");
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let rendered_text = frame
            .cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        assert!(rendered_text.contains("BT_APP_INPUT_OK"));
        pty.write(b"exit\r").unwrap();
        pty.shutdown().unwrap();
    }
    // ── T5: the drag's own clocks and rulings ──

    #[test]
    fn a_grabbed_tab_follows_the_hand_until_the_strip_runs_out() {
        // K115. The tab travels in x, and stops at the strip's own edges rather
        // than being carried out over the caption buttons.
        let viewport = [0.0, 900.0];
        let (slot_left, width) = (204.0_f32, 200.0);
        assert_eq!(
            grabbed_offset(slot_left, width, viewport, 300.0),
            96.0,
            "free of both edges, the offset is simply the distance"
        );
        assert_eq!(
            grabbed_offset(slot_left, width, viewport, -50.0),
            -slot_left,
            "held past the left edge it stops with its leading edge on it"
        );
        assert_eq!(
            grabbed_offset(slot_left, width, viewport, 5_000.0),
            700.0 - slot_left,
            "and past the right edge with its trailing edge on that one"
        );
        assert_eq!(
            grabbed_offset(0.0, 200.0, [0.0, 120.0], 5_000.0),
            0.0,
            "a viewport narrower than the tab keeps the leading edge, not the trailing one"
        );
    }

    #[test]
    fn a_flip_runs_the_displacement_down_to_nothing_on_the_grab_curve() {
        // K117/K118. One motion, 160ms, cubic-bezier(.2, 0, 0, 1).
        let now = Instant::now();
        let mut flip = FlipTween::default();
        assert_eq!(flip.sample(now, Motion::Full), (0.0, false));
        flip.displace(-96.0, now, Motion::Full);
        let (start, moving) = flip.sample(now, Motion::Full);
        assert!((start + 96.0).abs() < 1e-3, "it starts where the tab was");
        assert!(moving);
        let (mid, moving) = flip.sample(now + Duration::from_millis(80), Motion::Full);
        assert!(moving);
        assert!(
            mid > -96.0 && mid < 0.0,
            "and travels the whole way in between"
        );
        assert!(
            mid.abs() < 48.0,
            "the curve leaves fast: half the time is well past half the distance"
        );
        assert_eq!(
            TAB_FLIP,
            Duration::from_millis(160),
            "`transform .16s` (mock-up 6570)"
        );
        assert!(
            flip.sample(now + Duration::from_millis(159), Motion::Full)
                .1,
            "still moving one millisecond short of the end"
        );
        assert_eq!(
            flip.sample(now + Duration::from_millis(160), Motion::Full),
            (0.0, false),
            "at 160ms the tab is simply in its slot"
        );
    }

    #[test]
    fn a_tab_displaced_again_mid_slide_starts_from_where_it_actually_is() {
        // The mock-up measures `getBoundingClientRect`, which includes the live
        // transform (6579-6583): a second swap inside the first slide's 160ms
        // must not snap the tab back to a slot it never reached.
        let now = Instant::now();
        let mut flip = FlipTween::default();
        flip.displace(-96.0, now, Motion::Full);
        let at = now + Duration::from_millis(80);
        let (mid, _) = flip.sample(at, Motion::Full);
        flip.displace(-204.0, at, Motion::Full);
        let (restarted, _) = flip.sample(at, Motion::Full);
        assert!(
            (restarted - (mid - 204.0)).abs() < 1e-3,
            "the new slide starts at the old one's live position plus the new delta"
        );
    }

    #[test]
    fn reduced_motion_puts_a_displaced_tab_straight_into_its_slot() {
        // **Ruling.** The mock-up writes these transitions from JavaScript, where
        // no `prefers-reduced-motion` block can reach them, so its silence here
        // is its medium rather than a decision. A transform travelling across the
        // screen is precisely what the preference is about — unlike the progress
        // ring's sweep, which carries a reading and is deliberately left running.
        let now = Instant::now();
        let mut flip = FlipTween::default();
        flip.displace(-96.0, now, Motion::Reduced);
        assert_eq!(flip.sample(now, Motion::Reduced), (0.0, false));
    }

    #[test]
    fn the_landing_wash_runs_out_over_its_own_two_hundred_milliseconds() {
        // K121. Only the `from` is a design value, so only how much of it is left
        // is a state.
        let now = Instant::now();
        let mut landing = LandTween::default();
        assert_eq!(landing.sample(now, Motion::Full), (0.0, false));
        landing.start(now, Motion::Full);
        assert_eq!(landing.sample(now, Motion::Full), (1.0, true));
        let mut last = 1.0;
        for step in 1..20 {
            let (left, moving) =
                landing.sample(now + Duration::from_millis(step * 10), Motion::Full);
            assert!(left <= last, "the wash only ever fades");
            assert!(moving);
            last = left;
        }
        assert_eq!(
            TAB_LAND,
            Duration::from_millis(200),
            "`animation: tab-land .2s` (mock-up 967)"
        );
        assert!(
            landing
                .sample(now + Duration::from_millis(199), Motion::Full)
                .1
        );
        assert_eq!(
            landing.sample(now + Duration::from_millis(200), Motion::Full),
            (0.0, false)
        );
    }

    /// PIN — the profile picker's arrow turns over across 140ms on
    /// `cubic-bezier(.2,0,0,1)`, and it is the turn that is drawn rather than
    /// its two ends.
    ///
    /// `.chevbtn svg { transition: transform 140ms cubic-bezier(.2,0,0,1) }`
    /// (mock-up 415-418). Both halves of that declaration are load-bearing and
    /// both are pinned here against the ways they get quietly deleted: cut the
    /// duration to nothing and the arrow arrives before the first sample, so
    /// there is no midpoint left to read; swap the curve for `linear` and the
    /// midpoint lands at half a turn instead of where this curve actually puts
    /// it, which — because `.2,0,0,1` front-loads almost everything and then
    /// crawls — is nearly nine tenths of the way over.
    #[test]
    fn the_profile_chevron_turns_over_across_a_hundred_and_forty_milliseconds() {
        assert_eq!(
            CHEVRON_TURN,
            Duration::from_millis(140),
            "`transition: transform 140ms` (mock-up 417)"
        );
        assert_eq!(
            GRAB_EASE,
            [0.2, 0.0, 0.0, 1.0],
            "`cubic-bezier(.2,0,0,1)` (mock-up 417)"
        );

        let now = Instant::now();
        let mut turn = ChevronTurn::default();
        assert_eq!(
            turn.sample(now, Motion::Full),
            (0.0, false),
            "an untouched picker's arrow points down and is not moving"
        );

        turn.retarget(true, now, Motion::Full);
        assert_eq!(turn.sample(now, Motion::Full), (0.0, true));

        // The middle of the transition is a real place, and it is where this
        // curve puts it rather than where a straight line would.
        let (halfway, moving) = turn.sample(now + Duration::from_millis(70), Motion::Full);
        assert!(moving);
        assert!(
            halfway > 0.0 && halfway < 1.0,
            "70ms into a 140ms turn the arrow is partway over, saw {halfway}"
        );
        let eased = cubic_bezier(0.5, GRAB_EASE);
        assert!(
            (halfway - eased).abs() < 1e-3,
            "the turn is drawn on its own curve: expected {eased}, saw {halfway}"
        );
        assert!(
            (halfway - 0.5).abs() > 0.2,
            "halfway in time is not halfway over on this curve — saw {halfway}, \
             which is what `linear` would have given"
        );

        // It only ever goes forwards, and it stops.
        let mut last = 0.0;
        for step in 0..=14 {
            let (at, _) = turn.sample(now + Duration::from_millis(step * 10), Motion::Full);
            assert!(at >= last, "the arrow does not turn back on its way over");
            last = at;
        }
        assert!(
            turn.sample(now + Duration::from_millis(139), Motion::Full)
                .1
        );
        assert_eq!(
            turn.sample(now + Duration::from_millis(140), Motion::Full),
            (1.0, false),
            "at 140ms the arrow has arrived and owes no more frames"
        );
        assert_eq!(
            turn.sample(now + Duration::from_secs(9), Motion::Full),
            (1.0, false)
        );
    }

    /// PIN — a turn reversed mid-flight carries on from the angle the arrow is
    /// actually at.
    ///
    /// This is what a CSS transition does to a property whose target changes
    /// while it is running, and it is the whole difference between a control
    /// that turns and one that flickers: clicking the picker twice quickly must
    /// not snap the arrow to an end it never reached and then run back from
    /// there. Red gate: the sample taken at the instant of the reversal is the
    /// same number on both sides of it.
    #[test]
    fn the_chevron_reverses_from_where_the_arrow_actually_is() {
        let now = Instant::now();
        let mut turn = ChevronTurn::default();
        turn.retarget(true, now, Motion::Full);

        let reversed_at = now + Duration::from_millis(40);
        let (mid, _) = turn.sample(reversed_at, Motion::Full);
        assert!(mid > 0.0 && mid < 1.0, "caught mid-turn, saw {mid}");

        turn.retarget(false, reversed_at, Motion::Full);
        let (restarted, moving) = turn.sample(reversed_at, Motion::Full);
        assert!(moving);
        assert!(
            (restarted - mid).abs() < 1e-6,
            "the arrow jumped from {mid} to {restarted} when it was told to come back"
        );

        // And from there it goes the other way, all the way home.
        let mut last = restarted;
        for step in 1..=14 {
            let (at, _) = turn.sample(reversed_at + Duration::from_millis(step * 10), Motion::Full);
            assert!(
                at <= last,
                "the reversed turn went further over instead of coming back"
            );
            last = at;
        }
        assert_eq!(
            turn.sample(reversed_at + CHEVRON_TURN, Motion::Full),
            (0.0, false)
        );

        // Told again what it is already doing, it does not restart: a caller
        // that re-reports the same state must not stretch the transition.
        let mut steady = ChevronTurn::default();
        steady.retarget(true, now, Motion::Full);
        let at = now + Duration::from_millis(100);
        let (before, _) = steady.sample(at, Motion::Full);
        steady.retarget(true, at, Motion::Full);
        assert_eq!(steady.sample(at, Motion::Full).0, before);
        assert_eq!(
            steady.sample(now + CHEVRON_TURN, Motion::Full),
            (1.0, false)
        );
    }

    /// PIN — `@media (prefers-reduced-motion: reduce) { .chevbtn svg {
    /// transition: none } }` (mock-up 420).
    ///
    /// `none` is not a faster transition: there are no intermediate frames at
    /// all, the arrow is simply already over, and — the half that actually
    /// costs something — nothing asks to be woken up to draw the frames that do
    /// not exist.
    #[test]
    fn reduced_motion_turns_the_chevron_over_with_no_frames_in_between() {
        let now = Instant::now();
        let mut turn = ChevronTurn::default();
        turn.retarget(true, now, Motion::Reduced);
        assert_eq!(
            turn.sample(now, Motion::Reduced),
            (1.0, false),
            "under reduced motion the arrow is over the instant the list is up"
        );
        for step in 0..=14 {
            assert_eq!(
                turn.sample(now + Duration::from_millis(step * 10), Motion::Reduced),
                (1.0, false),
                "and there is never a frame of it on the way"
            );
        }
        turn.retarget(false, now + Duration::from_millis(50), Motion::Reduced);
        assert_eq!(
            turn.sample(now + Duration::from_millis(50), Motion::Reduced),
            (0.0, false)
        );
    }

    /// PIN — the turn pays the strip's frame debt in the mark's own quantized
    /// angles, so it draws every step it has and stops the moment it lands.
    ///
    /// Two failures this stands against, and they pull opposite ways. Compare
    /// the raw fraction and every wake-up of the 140ms owes a present, including
    /// the long tail where `.2,0,0,1` is crawling through less than a degree and
    /// the rasterized arrow is byte-identical. Compare nothing at all and the
    /// arrow strands on whatever frame the last present happened to catch —
    /// which is the failure `tab_owes_frame` was written for, in its original
    /// half-faded-icon form.
    #[test]
    fn the_chevron_s_frame_debt_is_paid_in_drawn_angles() {
        let now = Instant::now();
        let mut turn = ChevronTurn::default();
        turn.retarget(true, now, Motion::Full);

        // The strip wakes on its own beat; what it draws each time is the mark.
        let mut last_drawn: Option<marks::ChromeMark> = None;
        let mut presents = 0_u32;
        let mut wakes = 0_u32;
        let mut drawn = Vec::new();
        let mut at = now;
        loop {
            let (fraction, moving) = turn.sample(at, Motion::Full);
            let showing = marks::ChromeMark::chevron(fraction);
            if tab_owes_frame(last_drawn, showing) {
                last_drawn = Some(showing);
                presents += 1;
                drawn.push(showing);
            }
            wakes += 1;
            if !moving {
                break;
            }
            at += STRIP_ANIMATION_FRAME;
        }

        assert!(
            wakes > presents,
            "every single wake-up presented a frame ({presents} of {wakes}) — the debt \
             is being measured on something finer than the arrow is drawn at"
        );
        assert!(
            presents >= 5,
            "only {presents} frames of the turn were ever drawn — that is a swap \
             wearing an animation's clothes"
        );
        assert!(
            presents <= u32::from(marks::CHEVRON_TURN_STEPS),
            "a single turn asked for {presents} rasters, more than the quantum allows"
        );
        assert_eq!(
            drawn.first().copied(),
            Some(marks::ChromeMark::chevron(0.0)),
            "the turn is drawn from the arrow's resting angle"
        );
        assert_eq!(
            drawn.last().copied(),
            Some(marks::ChromeMark::chevron(1.0)),
            "and the frame it settles on is the terminal one — an arrow left at 175° is \
             the stranded-mid-breath bug wearing a different mark"
        );
        for pair in drawn.windows(2) {
            assert_ne!(pair[0], pair[1], "a present that redrew the same angle");
        }
    }

    #[test]
    fn reduced_motion_skips_the_landing_animation_outright() {
        // Mock-up 968 says so in as many words, and an animation with only a
        // `from` has nothing to hold: off means the tab, unwashed.
        let now = Instant::now();
        let mut landing = LandTween::default();
        landing.start(now, Motion::Reduced);
        assert_eq!(landing.sample(now, Motion::Reduced), (0.0, false));
    }

    #[test]
    fn a_cancelled_drag_puts_the_tab_back_without_crossing_the_pinned_seam() {
        // F57, applied to the one move geometry did not choose (K128's restore).
        let pinned = [true, true, false, false];
        assert_eq!(
            partition_clamped(&pinned, 3, 2),
            2,
            "inside its own partition the restore reaches its slot"
        );
        assert_eq!(
            partition_clamped(&pinned, 3, 0),
            2,
            "and stops at the seam rather than landing among the pinned tabs"
        );
        assert_eq!(partition_clamped(&pinned, 0, 3), 1);
        assert_eq!(partition_clamped(&pinned, 2, 2), 2, "a move to nowhere");
        assert_eq!(
            partition_clamped(&[false, false, false], 2, 0),
            0,
            "with no seam there is nothing to stop at"
        );
    }

    #[test]
    fn the_pointer_keeps_one_shape_for_the_whole_drag() {
        // K113. "The cursor changing shape mid-drag would say something happened
        // when nothing did" (mock-up 1710-1711).
        use winit::window::CursorIcon;
        assert_eq!(pointer_cursor(false, None), CursorIcon::Default);
        assert_eq!(
            pointer_cursor(false, Some(bt_layout::Axis::Row)),
            CursorIcon::EwResize
        );
        assert_eq!(
            pointer_cursor(false, Some(bt_layout::Axis::Col)),
            CursorIcon::NsResize
        );
        for axis in [None, Some(bt_layout::Axis::Row), Some(bt_layout::Axis::Col)] {
            assert_eq!(
                pointer_cursor(true, axis),
                CursorIcon::Default,
                "a tab drag crossing a divider must not flicker into a resize arrow"
            );
        }
    }

    #[test]
    fn a_drag_that_never_starts_leaves_the_press_exactly_as_t4_left_it() {
        // The seam between T4 and T5, from T5's side: the 6px and the identity
        // are the press's, and the move it names as the drag's first is the
        // only thing this slice reads.
        let now = Instant::now();
        let mut press = TabPress::armed(TabId(1), PhysicalPosition::new(100.0, 20.0), now);
        assert!(!press.travelled(PhysicalPosition::new(105.0, 20.0), 1.0));
        assert_eq!(press.promise, TabPressPromise::Pending);
        assert!(press.travelled(PhysicalPosition::new(106.0, 20.0), 1.0));
        assert_eq!(press.promise, TabPressPromise::Slipped);
        assert!(
            !press.travelled(PhysicalPosition::new(300.0, 20.0), 1.0),
            "the drag starts once — every later move is the gesture, not its start"
        );
    }

    // ── U4: the engine tabs and panes share (J111-J122) ──

    /// A drag carrying `source`, with `landing` last surveyed. The carry is a
    /// tab's when the source is a tab, because that pairing is the engine's
    /// invariant rather than a choice any caller makes.
    fn drag_of(source: DragSource, landing: Option<DropLanding>) -> Drag {
        Drag {
            source,
            carry: match source {
                DragSource::Tab(tab) => DragCarry::Tab(TabCarry {
                    grab_dx: 0.0,
                    origin: 0,
                    offset: 0.0,
                    moved: false,
                    home: tab,
                }),
                DragSource::Pane(_) => DragCarry::Pane,
            },
            pointer: PhysicalPosition::new(0.0, 0.0),
            landing,
        }
    }

    /// J113 — one threshold, and it belongs to the latch rather than to either
    /// press that owns one.
    ///
    /// The tab's half of this is already pinned by
    /// `travelling_past_six_pixels_abandons_the_delayed_switch`; what is new is
    /// that a pane head crosses the *same* six pixels, measured the same way, at
    /// every scale.
    ///
    /// Red gate: give the pane its own constant — any other number — and the
    /// middle assertion of each pass fails, because 6.0 is the only radius that
    /// is short of at 5px and past at 7px.
    #[test]
    fn a_pane_head_and_a_tab_cross_the_same_six_pixels() {
        for scale in [1.0_f64, 1.5, 2.0] {
            let origin = PhysicalPosition::new(100.0, 200.0);
            let mut pane = DragLatch::new(origin);
            let mut tab = TabPress::armed(TabId(1), origin, Instant::now());
            let short = PhysicalPosition::new(100.0 + 5.0 * scale, 200.0);
            let far = PhysicalPosition::new(100.0 + 7.0 * scale, 200.0);
            assert!(
                !pane.travelled(short, scale),
                "5 logical px is still a press"
            );
            assert!(!tab.travelled(short, scale), "and the tab agrees");
            assert!(pane.travelled(far, scale), "7 logical px is a drag");
            assert!(tab.travelled(far, scale), "and the tab agrees");
            assert!(
                !pane.travelled(PhysicalPosition::new(900.0, 900.0), scale),
                "the drag starts once, for a pane exactly as for a tab"
            );
        }
        // Euclidean, not per-axis: a diagonal hand travels as far as a straight
        // one. 4/4 is 5.66 and short; 5/5 is 7.07 and past.
        let mut latch = DragLatch::new(PhysicalPosition::new(0.0, 0.0));
        assert!(!latch.travelled(PhysicalPosition::new(4.0, 4.0), 1.0));
        assert!(latch.travelled(PhysicalPosition::new(5.0, 5.0), 1.0));
    }

    /// J120 — a release that landed nowhere goes home, and it is not the same
    /// answer as a release that landed.
    ///
    /// Red gate: the mock-up's own behaviour is `DragRelease::Commit` for both
    /// arms — `!d.target` falls in beside `d.target.reordered` and returns
    /// without undoing the live reorder (7202-7208). Return `Commit` for `None`
    /// here and this is the assertion that says the ruling was dropped.
    #[test]
    fn a_release_that_landed_nowhere_sends_the_gesture_home_rather_than_committing() {
        assert_eq!(release_verdict(None), DragRelease::Home);
        assert_eq!(
            release_verdict(Some(DropLanding::StripReorder { slot: 3 })),
            DragRelease::Commit,
            "the strip already applied it — committing is letting it stand"
        );
    }

    /// J116/K124 — the ghost stands down when the landing is already showing the
    /// user what it means.
    ///
    /// "The tab itself is the feedback" (mock-up 6837). A tab reordering in the
    /// strip is holding the slot it would take, so a second label saying the same
    /// name under the pointer is the drag telling you twice — and the one in the
    /// strip is the one telling you *where*. A pane has no such stand-in: nothing
    /// on screen moves for it, so the ghost is the entire report.
    ///
    /// Red gate: make `shows_itself` answer `false` and the first assertion
    /// fails; drop the `!` from `ghost_is_shown` and the second does.
    #[test]
    fn the_ghost_yields_to_a_landing_that_is_already_showing_itself() {
        let tab = drag_of(
            DragSource::Tab(TabId(1)),
            Some(DropLanding::StripReorder { slot: 0 }),
        );
        assert!(
            !tab.ghost_is_shown(),
            "the reordering tab is its own feedback"
        );
        let pane = drag_of(DragSource::Pane(bt_layout::SeatId(1)), None);
        assert!(
            pane.ghost_is_shown(),
            "nothing else on screen has moved for a pane, so the ghost is all there is"
        );
        assert!(
            drag_of(DragSource::Tab(TabId(1)), None).ghost_is_shown(),
            "a tab with nowhere to land is carried by the ghost like anything else"
        );
    }

    /// J111/J118 — one drag, two things it can be carrying, and a pane carries
    /// nothing that would let it be sent home.
    ///
    /// This is the guard `settle_home` reads, stated where it can be seen: a
    /// pane's J120 is a no-op *because* there is no carry to unwind, and there is
    /// no carry because the tree was never touched. The two are the same fact.
    ///
    /// Red gate: fold `TabCarry`'s four fields onto every drag — one struct, four
    /// `Option`s or four zeros — and `tab_carry()` answers `Some` for a pane.
    /// `settle_home` then reads `origin: 0` off it and walks whichever tab
    /// happens to be in slot 0 across the strip, on a gesture that was carrying a
    /// pane and never named a tab at all.
    #[test]
    fn a_pane_drag_carries_no_slot_and_no_offset_so_it_has_no_way_home() {
        let pane = drag_of(DragSource::Pane(bt_layout::SeatId(7)), None);
        assert_eq!(pane.tab(), None);
        assert!(
            pane.tab_carry().is_none(),
            "nothing moved, so nothing has to move back"
        );
        let tab = drag_of(DragSource::Tab(TabId(4)), None);
        assert_eq!(tab.tab(), Some(TabId(4)));
        assert!(tab.tab_carry().is_some());
    }

    // ── U5: what the pointer has found (K123-K135) ──

    /// **K135 — never onto yourself, in any zone.**
    ///
    /// Splitting a pane against itself and swapping it with itself are both the
    /// identity, so the honest report of a gesture that would do nothing is that
    /// there is nothing under the pointer. Note it is the *whole* pane that goes
    /// dead and not only its middle: `drag.leafId === leafId` is tested after the
    /// zone is computed and ignores it (7101).
    ///
    /// Red gate: drop the identity test and a pane held over its own left third
    /// answers `SeatEdge`, which U7 would turn into a split of a seat against
    /// itself.
    #[test]
    fn a_pane_has_no_landing_anywhere_on_itself() {
        let mine = bt_layout::SeatId(3);
        let other = bt_layout::SeatId(4);
        let held = DragSource::Pane(mine);
        for aim in [
            seats::LayoutAim::SeatEdge(mine, seats::DropEdge::Left),
            seats::LayoutAim::SeatEdge(mine, seats::DropEdge::Bottom),
            seats::LayoutAim::SeatCentre(mine),
        ] {
            assert_eq!(
                landing_for_aim(held, aim),
                None,
                "a pane held over its own rectangle has no landing: {aim:?}"
            );
        }
        assert_eq!(
            landing_for_aim(held, seats::LayoutAim::SeatCentre(other)),
            Some(DropLanding::SeatCentre { target: other }),
            "a neighbour is a target like any other"
        );
        assert_eq!(
            landing_for_aim(
                DragSource::Tab(TabId(1)),
                seats::LayoutAim::SeatCentre(mine)
            ),
            Some(DropLanding::SeatCentre { target: mine }),
            "a tab is not any pane, so no pane is its own"
        );
    }

    /// **K130/G83 — the rim belongs to no seat, so it can never be your own.**
    ///
    /// Dragging your own pane out to the rim is precisely how you ask for it to
    /// sit beside everything else, which is the gesture G82 exists to give and
    /// the one the root split had no edge to offer before.
    ///
    /// Red gate: run the identity test before the match instead of inside its two
    /// seat arms and the rim goes dead for pane drags — the only source that has
    /// any use for it.
    #[test]
    fn the_rim_is_no_ones_pane() {
        for edge in seats::DropEdge::NEAREST_FIRST {
            assert_eq!(
                landing_for_aim(
                    DragSource::Pane(bt_layout::SeatId(1)),
                    seats::LayoutAim::Rim(edge)
                ),
                Some(DropLanding::RootRim { edge })
            );
        }
    }

    /// **The two strip landings yield the ghost; the three layout ones do not.**
    ///
    /// The mock-up sets `drag.ghost.style.opacity = "0"` inside the strip and
    /// says why beside the *pane* case: "the preview in the strip is the ghost
    /// now" (6792). Out over the layout the preview is a box drawn somewhere
    /// else, saying something the ghost does not, so both are on screen at once.
    ///
    /// Red gate: give `StripExtract` the layout's answer and a pane torn towards
    /// the strip is labelled twice, once under the pointer and once in the slot;
    /// give a rim the strip's answer and the hand goes empty over the layout.
    #[test]
    fn only_the_strip_takes_the_ghosts_place() {
        assert!(DropLanding::StripReorder { slot: 0 }.shows_itself());
        assert!(DropLanding::StripExtract { slot: 2 }.shows_itself());
        assert!(
            !DropLanding::RootRim {
                edge: seats::DropEdge::Left
            }
            .shows_itself()
        );
        assert!(
            !DropLanding::SeatEdge {
                target: bt_layout::SeatId(1),
                edge: seats::DropEdge::Top
            }
            .shows_itself()
        );
        assert!(
            !DropLanding::SeatCentre {
                target: bt_layout::SeatId(1)
            }
            .shows_itself()
        );
    }

    /// **U5 commits nothing it did not already commit.**
    ///
    /// **U7 — the commit table, all five landings** (mock-up 7202-7231).
    ///
    /// Three answers, and each one is a different relationship to work: the strip
    /// reorder is *kept* (it happened live, slot by slot), the three landings in
    /// the layout are *performed* now (the tree was untouched all gesture), and
    /// what is left goes home having decided nothing.
    ///
    /// `StripExtract` is the one landing that sits with the empty hand, and its
    /// reason is not that N157 is unwritten but that a torn-out pane needs a tab
    /// to arrive in — a tab being a tree *and a shell* in this build. It is also
    /// no longer offered by the survey, so this arm answers about a landing that
    /// cannot be reached; the verdict is a fact about the landing rather than
    /// about which of them the pointer can produce.
    ///
    /// Red gate: this test replaced U5's `the_landings_without_a_verb_yet_all_go
    /// _home`, which asserted `Home` for all four. Send any of the three layout
    /// landings back to `Home` and the drop goes silently missing behind a
    /// preview that promised it.
    #[test]
    fn the_release_table_keeps_the_strip_performs_the_layout_and_sends_the_rest_home() {
        assert_eq!(
            release_verdict(Some(DropLanding::StripReorder { slot: 3 })),
            DragRelease::Commit,
            "the strip already did it"
        );
        for landing in [
            DropLanding::RootRim {
                edge: seats::DropEdge::Bottom,
            },
            DropLanding::SeatEdge {
                target: bt_layout::SeatId(2),
                edge: seats::DropEdge::Right,
            },
            DropLanding::SeatCentre {
                target: bt_layout::SeatId(2),
            },
        ] {
            assert_eq!(
                release_verdict(Some(landing)),
                DragRelease::Land,
                "{landing:?} is a drop, and letting go performs it"
            );
        }
        assert_eq!(
            release_verdict(Some(DropLanding::StripExtract { slot: 1 })),
            DragRelease::Home,
            "a pane torn into the strip needs a tab to land in, and a tab is a shell"
        );
        assert_eq!(release_verdict(None), DragRelease::Home);
    }

    /// **A tab is a tree and a shell, so a tree with any other number of terminal
    /// leaves is not one.**
    ///
    /// The count is what matters and neither direction is benign. Two terminal
    /// leaves gives the second one no grid at all — the frame pipeline draws one
    /// `ViewportFrame` per present, placed at `seats.terminal()`'s rectangle — so
    /// it comes out as a pane head over a blank body, with not even the notice a
    /// `Placeholder` would earn. That is I106's crash with the volume turned
    /// down. Zero gives a tab with nothing running in it.
    ///
    /// Every other kind is invisible to the question: a preview and a files
    /// column are panes a tab already holds today.
    ///
    /// Red gate: write this as `>= 1` and a tab merged onto a pane is drawn as a
    /// legal drop; write it as `<= 1` and the ordinary two-pane tab stops being
    /// hostable and every drop refuses.
    #[test]
    fn a_tab_is_a_tree_with_exactly_one_terminal_in_it() {
        let seat = |id: u64, kind: bt_layout::SeatKind| {
            LayoutNode::seat(bt_layout::Seat::new(bt_layout::SeatId(id), kind))
        };
        let row = |a: LayoutNode, b: LayoutNode| {
            LayoutNode::split(bt_layout::SplitId(1), Axis::Row, a, b)
        };
        let term = |id| seat(id, bt_layout::SeatKind::Terminal);

        assert!(tab_can_host(&term(1)), "the ordinary lone terminal");
        assert!(
            tab_can_host(&row(term(1), seat(2, bt_layout::SeatKind::Preview))),
            "a terminal beside its preview is today's two-pane tab"
        );
        assert!(
            tab_can_host(&row(term(1), seat(2, bt_layout::SeatKind::Files))),
            "and a files column is a pane like any other"
        );
        assert!(
            !tab_can_host(&row(term(1), term(2))),
            "the second terminal would be a pane with no session behind it"
        );
        assert!(
            !tab_can_host(&row(
                seat(1, bt_layout::SeatKind::Preview),
                seat(2, bt_layout::SeatKind::Files)
            )),
            "a tab with nothing running is not a tab"
        );
    }

    // ── U6: the plan, its cache, and the word in the box (M148, M155, L137) ──

    fn inputs_of(landing: DropLanding, source: DragSource) -> PlanInputs {
        PlanInputs {
            landing,
            source,
            tree: LayoutNode::seat(bt_layout::Seat::new(
                bt_layout::SeatId(1),
                bt_layout::SeatKind::Terminal,
            )),
            cargo: None,
            viewport: LogicalRect::from_px(1600, 900),
            scale_ppm: 1_000_000,
        }
    }

    /// **The cache is over a computation, and its key is every input the
    /// computation has.**
    ///
    /// "Re-plan only when the landing moves" is the behaviour, but the landing is
    /// not the whole question: a window resized under a still pointer, or a DPI
    /// change mid-drag, changes the layout the plan describes without the pointer
    /// moving at all. Keying on the landing alone leaves the promise on screen
    /// describing a layout that no longer exists — a stale promise is exactly what
    /// M148 is about, arrived at from the other direction.
    #[test]
    fn the_plan_stands_while_its_question_does_and_falls_when_anything_moves() {
        let pane = DragSource::Pane(bt_layout::SeatId(1));
        let landing = DropLanding::SeatEdge {
            target: bt_layout::SeatId(2),
            edge: seats::DropEdge::Right,
        };
        let base = inputs_of(landing, pane);
        assert_eq!(base, inputs_of(landing, pane), "the same question, twice");

        let moved_zone = inputs_of(
            DropLanding::SeatEdge {
                target: bt_layout::SeatId(2),
                edge: seats::DropEdge::Left,
            },
            pane,
        );
        assert_ne!(
            base, moved_zone,
            "the other side of the same pane is a new plan"
        );

        let mut resized = inputs_of(landing, pane);
        resized.viewport = LogicalRect::from_px(1200, 900);
        assert_ne!(
            base, resized,
            "a resize re-plans without the pointer moving"
        );

        let mut rescaled = inputs_of(landing, pane);
        rescaled.scale_ppm = 1_500_000;
        assert_ne!(base, rescaled, "and so does a DPI change");

        let mut edited = inputs_of(landing, pane);
        edited.tree = LayoutNode::split(
            bt_layout::SplitId(1),
            Axis::Row,
            LayoutNode::seat(bt_layout::Seat::new(
                bt_layout::SeatId(1),
                bt_layout::SeatKind::Terminal,
            )),
            LayoutNode::seat(bt_layout::Seat::new(
                bt_layout::SeatId(2),
                bt_layout::SeatKind::Terminal,
            )),
        );
        assert_ne!(base, edited, "a tree that changed under the drag re-plans");

        let carried = inputs_of(landing, DragSource::Tab(TabId(7)));
        assert_ne!(
            base, carried,
            "the same zone means a different thing depending on what is in the hand"
        );
    }

    /// **L137 — the centre says its name, and nothing else does.**
    ///
    /// The centre's box is the same rectangle an edge's is; a pane swaps payloads
    /// with the target and a tab takes its place outright, and the geometry cannot
    /// tell you which. Every other zone answers with nothing, because its shape
    /// has already spoken.
    #[test]
    fn only_the_centre_says_a_word_and_it_depends_on_the_hand() {
        let target = bt_layout::SeatId(2);
        let pane = DragSource::Pane(bt_layout::SeatId(1));
        let tab = DragSource::Tab(TabId(1));
        assert_eq!(
            DropLanding::SeatCentre { target }.caption(pane),
            "Swap panes"
        );
        assert_eq!(
            DropLanding::SeatCentre { target }.caption(tab),
            "Replace pane"
        );
        for landing in [
            DropLanding::SeatEdge {
                target,
                edge: seats::DropEdge::Right,
            },
            DropLanding::RootRim {
                edge: seats::DropEdge::Top,
            },
            DropLanding::StripExtract { slot: 0 },
            DropLanding::StripReorder { slot: 0 },
        ] {
            assert_eq!(
                landing.caption(pane),
                "",
                "{landing:?} draws its own meaning"
            );
            assert_eq!(landing.caption(tab), "");
        }
    }

    /// A landing's aim is the one it was read off — and the strip's two never had
    /// one, which is what keeps the dock box off the strip entirely.
    #[test]
    fn a_landings_aim_is_the_aim_it_came_from() {
        let seat = bt_layout::SeatId(4);
        for aim in [
            seats::LayoutAim::Rim(seats::DropEdge::Bottom),
            seats::LayoutAim::SeatEdge(seat, seats::DropEdge::Left),
            seats::LayoutAim::SeatCentre(seat),
        ] {
            let landing = landing_for_aim(DragSource::Tab(TabId(1)), aim)
                .expect("a tab may land on any of them");
            assert_eq!(landing.layout_aim(), Some(aim));
        }
        assert_eq!(DropLanding::StripExtract { slot: 2 }.layout_aim(), None);
        assert_eq!(DropLanding::StripReorder { slot: 2 }.layout_aim(), None);
    }

    /// **The box is a state, and under reduced motion it is nothing else.**
    ///
    /// The mock-up gives `#dock-preview` one transition and one only — `opacity
    /// .1s` — and the geometry transitions beside it are unreachable by
    /// construction: the box's rectangle is a function of the promise
    /// (`zone:fits`), so any move of the box is a change of the promise, and
    /// `promise()` puts `.snap` on before the new geometry lands. A glide could
    /// only run while the answer stayed the same and the box moved anyway, which
    /// within one drag cannot happen. So the box snaps, always — which is M148
    /// satisfied by an implementation that has no way to lag.
    ///
    /// What is left is the fade, and this is its span and its reduced-motion
    /// answer. The 160ms next door is the pin's; borrowing it would be the wrong
    /// number arrived at silently.
    #[test]
    fn the_dock_box_fades_over_its_own_hundred_milliseconds_and_not_at_all_reduced() {
        assert_eq!(
            DOCK_PREVIEW_FADE,
            Duration::from_millis(100),
            "`transition: opacity .1s ease`"
        );
        assert_ne!(
            DOCK_PREVIEW_FADE,
            Duration::from_millis(bt_render::WINDOW_TAB_PIN_REVEAL_MS),
            "the dock box does not fade on the pin's clock"
        );

        let now = Instant::now();
        let mut reduced = RevealTween::over(DOCK_PREVIEW_FADE);
        reduced.retarget(1.0, now, Motion::Reduced);
        assert_eq!(
            reduced.sample(now, Motion::Reduced),
            (1.0, false),
            "reduced motion makes the box a state: it is there, and nothing moved"
        );

        let mut full = RevealTween::over(DOCK_PREVIEW_FADE);
        full.retarget(1.0, now, Motion::Full);
        let (opening, moving) = full.sample(now + Duration::from_millis(20), Motion::Full);
        assert!(
            moving && opening > 0.0 && opening < 1.0,
            "mid-fade: {opening}"
        );
        assert_eq!(
            full.sample(now + DOCK_PREVIEW_FADE, Motion::Full),
            (1.0, false),
            "and it is done at its own span, not before and not after"
        );
    }

    /// **M147 — only the aimed-at pane can be traced**, and the rim has none.
    #[test]
    fn a_refusal_points_at_a_pane_only_when_the_gesture_had_one() {
        let seat = bt_layout::SeatId(3);
        assert_eq!(
            DropLanding::SeatEdge {
                target: seat,
                edge: seats::DropEdge::Top,
            }
            .aimed_at(),
            Some(seat)
        );
        assert_eq!(
            DropLanding::SeatCentre { target: seat }.aimed_at(),
            Some(seat)
        );
        assert_eq!(
            DropLanding::RootRim {
                edge: seats::DropEdge::Top,
            }
            .aimed_at(),
            None,
            "the rim asked to divide the whole layout, so there is no pane to name"
        );
    }
}
